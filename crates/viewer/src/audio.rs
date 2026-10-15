//! Sound: `SoundHandler`, `SoundManager` and the OpenAL mix in one place. Sound events resolve
//! through the local Minecraft assets (`indexes/1.12.json` and `minecraft/sounds.json`), the Ogg
//! files decode with lewton, and an own mixer plays them: linear distance attenuation over
//! `16 * max(volume, 1)` blocks as OpenAL's linear model with paulscode's settings, a pan by the
//! listener's right vector in place of OpenAL's 3D panning, and pitch by resampling.
//!
//! The mix goes to the default output device (cpal), or in a streamed session to the server as
//! 48 kHz PCM (link.rs). Smoke runs stay silent. Nothing read from the assets touches disk.

use std::collections::HashMap;
use std::fs::File;
use std::io::{self, BufReader};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use bevy::prelude::*;
use lewton::inside_ogg::OggStreamReader;
use serde_json::Value;
use worldgen::JavaRandom;

/// `SoundCategory`, with the options file's names.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Category {
    Master,
    Music,
    Blocks,
    Players,
    Ambient,
}

impl Category {
    pub const ALL: [Category; 5] = [Category::Master, Category::Music, Category::Blocks, Category::Players, Category::Ambient];
}

/// Decoded PCM, interleaved `i16`.
pub struct Pcm {
    pub channels: usize,
    pub rate: u32,
    pub samples: Vec<i16>,
}

impl Pcm {
    fn frames(&self) -> usize {
        self.samples.len() / self.channels.max(1)
    }
}

fn decode(path: &Path) -> io::Result<Pcm> {
    let mut r = OggStreamReader::new(BufReader::new(File::open(path)?)).map_err(|e| io::Error::other(e.to_string()))?;
    let channels = r.ident_hdr.audio_channels as usize;
    let rate = r.ident_hdr.audio_sample_rate;
    let mut samples = Vec::new();
    while let Some(p) = r.read_dec_packet_itl().map_err(|e| io::Error::other(e.to_string()))? {
        samples.extend_from_slice(&p);
    }
    Ok(Pcm { channels, rate, samples })
}

fn seed() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos() as i64)
}

/// One entry of an event's `sounds` list (`Sound`).
#[derive(Clone, Debug)]
struct Entry {
    name: String,
    volume: f32,
    pitch: f32,
    weight: u32,
    stream: bool,
    /// `"type": "event"`: the name is another event.
    event: bool,
}

/// The asset index, the sound events, and the decoded sounds so far.
pub struct SoundAssets {
    objects: PathBuf,
    /// Asset path (`minecraft/sounds/dig/stone1.ogg`) to object hash.
    index: HashMap<String, String>,
    events: HashMap<String, Vec<Entry>>,
    cache: Mutex<HashMap<String, Arc<Pcm>>>,
}

/// Where the assets are looked for, in order: `MCRS_ASSETS`, Prism Launcher's, the vanilla
/// launcher's, then `%APPDATA%\mc-rs\assets`.
pub fn asset_candidates() -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Ok(p) = std::env::var("MCRS_ASSETS") {
        out.push(PathBuf::from(p));
    }
    if let Ok(a) = std::env::var("APPDATA") {
        let a = Path::new(&a);
        out.push(a.join("PrismLauncher").join("assets"));
        out.push(a.join(".minecraft").join("assets"));
    }
    if let Ok(home) = std::env::var("HOME") {
        out.push(Path::new(&home).join(".minecraft/assets"));
    }
    out.extend(paths::data_dir().map(|d| d.join("assets")));
    out
}

impl SoundAssets {
    pub fn find() -> Option<SoundAssets> {
        asset_candidates().into_iter().find_map(|root| SoundAssets::load(&root).ok())
    }

    pub fn load(root: &Path) -> io::Result<SoundAssets> {
        let index: Value = serde_json::from_reader(BufReader::new(File::open(root.join("indexes").join("1.12.json"))?))
            .map_err(|e| io::Error::other(e.to_string()))?;
        let mut map = HashMap::new();
        if let Some(objs) = index.get("objects").and_then(Value::as_object) {
            for (k, v) in objs {
                if let Some(h) = v.get("hash").and_then(Value::as_str) {
                    map.insert(k.clone(), h.to_string());
                }
            }
        }
        let objects = root.join("objects");
        let sounds_hash = map.get("minecraft/sounds.json").ok_or_else(|| io::Error::other("no sounds.json in the index"))?;
        let sounds: Value = serde_json::from_reader(BufReader::new(File::open(objects.join(&sounds_hash[..2]).join(sounds_hash))?))
            .map_err(|e| io::Error::other(e.to_string()))?;
        Ok(SoundAssets { objects, index: map, events: parse_events(&sounds), cache: Mutex::new(HashMap::new()) })
    }

    fn path_of(&self, name: &str) -> Option<PathBuf> {
        let h = self.index.get(&format!("minecraft/sounds/{name}.ogg"))?;
        Some(self.objects.join(&h[..2]).join(h))
    }

    /// `SoundEventAccessor.cloneEntry`: a weighted pick, following event references.
    fn pick(&self, event: &str, rand: &mut JavaRandom, depth: u32) -> Option<Entry> {
        let list = self.events.get(event)?;
        let total: u32 = list.iter().map(|e| e.weight).sum();
        if total == 0 || depth > 8 {
            return None;
        }
        let mut w = rand.next_int_bounded(total as i32) as u32;
        for e in list {
            if w < e.weight {
                return if e.event { self.pick(&e.name, rand, depth + 1) } else { Some(e.clone()) };
            }
            w -= e.weight;
        }
        None
    }

    fn pcm(&self, name: &str) -> Option<Arc<Pcm>> {
        if let Some(p) = self.cache.lock().ok()?.get(name) {
            return Some(p.clone());
        }
        let pcm = Arc::new(decode(&self.path_of(name)?).ok()?);
        self.cache.lock().ok()?.insert(name.to_string(), pcm.clone());
        Some(pcm)
    }
}

fn parse_events(v: &Value) -> HashMap<String, Vec<Entry>> {
    let mut out = HashMap::new();
    let Some(obj) = v.as_object() else { return out };
    for (event, def) in obj {
        let mut list = Vec::new();
        for s in def.get("sounds").and_then(Value::as_array).into_iter().flatten() {
            let e = match s {
                Value::String(n) => Entry { name: n.clone(), volume: 1.0, pitch: 1.0, weight: 1, stream: false, event: false },
                Value::Object(o) => Entry {
                    name: o.get("name").and_then(Value::as_str).unwrap_or("").to_string(),
                    volume: o.get("volume").and_then(Value::as_f64).unwrap_or(1.0) as f32,
                    pitch: o.get("pitch").and_then(Value::as_f64).unwrap_or(1.0) as f32,
                    weight: o.get("weight").and_then(Value::as_u64).unwrap_or(1) as u32,
                    stream: o.get("stream").and_then(Value::as_bool).unwrap_or(false),
                    event: o.get("type").and_then(Value::as_str) == Some("event"),
                },
                _ => continue,
            };
            list.push(e);
        }
        out.insert(event.clone(), list);
    }
    out
}

/// What a voice reads from.
enum Source {
    Memory(Arc<Pcm>),
    /// Music: decoded a packet at a time, as paulscode streams it.
    Stream { reader: Box<OggStreamReader<BufReader<File>>>, channels: usize, rate: u32, buf: Vec<i16>, base: usize, done: bool },
}

impl Source {
    fn rate(&self) -> u32 {
        match self {
            Source::Memory(p) => p.rate,
            Source::Stream { rate, .. } => *rate,
        }
    }

    /// Frame `i` as (left, right) in -1..1; `None` past the end.
    fn frame(&mut self, i: usize) -> Option<(f32, f32)> {
        let s = |v: i16| v as f32 / 32768.0;
        match self {
            Source::Memory(p) => {
                if i >= p.frames() {
                    return None;
                }
                let c = p.channels;
                let l = s(p.samples[i * c]);
                Some((l, if c > 1 { s(p.samples[i * c + 1]) } else { l }))
            }
            Source::Stream { reader, channels, buf, base, done, .. } => {
                let c = (*channels).max(1);
                while i >= *base + buf.len() / c {
                    if *done {
                        return None;
                    }
                    // Keep the two frames behind `i` too: the mixer interpolates between frame i and
                    // i + 1, so it reads i again after asking for i + 1.
                    let keep_from = i.saturating_sub(2).saturating_sub(*base).min(buf.len() / c);
                    buf.drain(..keep_from * c);
                    *base += keep_from;
                    match reader.read_dec_packet_itl() {
                        Ok(Some(p)) => buf.extend_from_slice(&p),
                        _ => *done = true,
                    }
                }
                if i < *base {
                    return Some((0.0, 0.0));
                }
                let k = (i - *base) * c;
                let l = s(buf[k]);
                Some((l, if c > 1 { s(buf[k + 1]) } else { l }))
            }
        }
    }
}

struct Voice {
    id: u64,
    source: Source,
    pos: f64,
    pitch: f32,
    /// `getClampedVolume` before the master volume.
    gain: f32,
    category: Category,
    /// Positional voices fade to nothing at `range` blocks.
    at: Option<Vec3>,
    range: f32,
}

/// The mixer, shared between the game and the output.
pub struct Mixer {
    voices: Vec<Voice>,
    listener: Vec3,
    right: Vec3,
    volumes: HashMap<Category, f32>,
    pub rate: u32,
}

/// Voices at once, as paulscode's 28 normal channels plus the streams.
const MAX_VOICES: usize = 32;

impl Mixer {
    fn new(rate: u32) -> Mixer {
        Mixer { voices: Vec::new(), listener: Vec3::ZERO, right: Vec3::X, volumes: HashMap::new(), rate }
    }

    fn volume(&self, c: Category) -> f32 {
        *self.volumes.get(&c).unwrap_or(&1.0)
    }

    /// Mixes into interleaved stereo `out`, adding to what is there.
    pub fn mix(&mut self, out: &mut [f32]) {
        let master = self.volume(Category::Master);
        let (listener, right, rate) = (self.listener, self.right, self.rate as f64);
        let vols: HashMap<Category, f32> = Category::ALL.iter().map(|&c| (c, self.volume(c))).collect();
        self.voices.retain_mut(|v| {
            let mut gain = v.gain * vols[&v.category] * master;
            let (mut gl, mut gr) = (1.0f32, 1.0f32);
            if let Some(at) = v.at {
                let d = at - listener;
                let dist = d.length();
                gain *= (1.0 - dist / v.range).max(0.0);
                if dist > 1e-3 {
                    let x = (d / dist).dot(right);
                    gl = (1.0 - x).min(1.0);
                    gr = (1.0 + x).min(1.0);
                }
            }
            let step = v.pitch as f64 * v.source.rate() as f64 / rate;
            for o in out.chunks_exact_mut(2) {
                let i = v.pos as usize;
                let Some(a) = v.source.frame(i) else { return false };
                let b = v.source.frame(i + 1).unwrap_or(a);
                let t = (v.pos - i as f64) as f32;
                o[0] += (a.0 + (b.0 - a.0) * t) * gain * gl;
                o[1] += (a.1 + (b.1 - a.1) * t) * gain * gr;
                v.pos += step;
            }
            true
        });
    }
}

/// The game's handle on sound.
#[derive(Resource, Clone)]
pub struct Audio {
    assets: Option<Arc<SoundAssets>>,
    mixer: Arc<Mutex<Mixer>>,
    rand: Arc<Mutex<JavaRandom>>,
    next_id: Arc<Mutex<u64>>,
    /// Silent: no output was opened (smoke runs, no device).
    pub muted: bool,
}

/// A started sound, for asking whether it still plays.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SoundId(u64);

impl Audio {
    fn new(assets: Option<SoundAssets>, rate: u32, muted: bool) -> Audio {
        Audio {
            assets: assets.map(Arc::new),
            mixer: Arc::new(Mutex::new(Mixer::new(rate))),
            rand: Arc::new(Mutex::new(JavaRandom::new(seed()))),
            next_id: Arc::new(Mutex::new(1)),
            muted,
        }
    }

    /// No assets and no output, for tests and smoke runs.
    pub fn silent() -> Audio {
        Audio::new(None, 48_000, true)
    }

    /// Opens the default output device, or none in a smoke run.
    pub fn open(smoke: bool) -> (Audio, Option<OutputStream>) {
        if smoke {
            return (Audio::silent(), None);
        }
        let assets = SoundAssets::find();
        if assets.is_none() {
            warn!("no Minecraft 1.12 assets found; the game is silent (looked in {:?})", asset_candidates());
        }
        match OutputStream::open() {
            Some((stream, rate)) => {
                let audio = Audio::new(assets, rate, false);
                stream.attach(audio.mixer.clone());
                (audio, Some(stream))
            }
            None => (Audio::new(assets, 48_000, true), None),
        }
    }

    /// For a streamed session: the mix leaves as 48 kHz `i16` stereo through `sink`, 10 ms at a
    /// time.
    pub fn open_streamed(sink: impl Fn(Vec<u8>) -> bool + Send + 'static) -> Audio {
        let assets = SoundAssets::find();
        if assets.is_none() {
            warn!("no Minecraft 1.12 assets found; the session is silent");
        }
        let audio = Audio::new(assets, proto::AUDIO_RATE, false);
        let mixer = audio.mixer.clone();
        thread::Builder::new()
            .name("audio-stream".into())
            .spawn(move || {
                let start = Instant::now();
                let mut produced: u64 = 0;
                let mut buf = vec![0f32; 2 * 480];
                loop {
                    thread::sleep(Duration::from_millis(10));
                    let due = (start.elapsed().as_secs_f64() * proto::AUDIO_RATE as f64) as u64;
                    while produced + 480 <= due {
                        buf.iter_mut().for_each(|s| *s = 0.0);
                        if let Ok(mut m) = mixer.lock() {
                            m.mix(&mut buf);
                        }
                        let bytes: Vec<u8> = buf.iter().flat_map(|s| ((s.clamp(-1.0, 1.0) * 32767.0) as i16).to_le_bytes()).collect();
                        if !sink(bytes) {
                            return;
                        }
                        produced += 480;
                    }
                }
            })
            .expect("spawn audio-stream");
        audio
    }

    /// `rand.nextFloat()` for the pitch spreads the game adds.
    pub fn next_float(&self) -> f32 {
        self.rand.lock().map_or(0.5, |mut r| r.next_float())
    }

    pub fn set_listener(&self, eye: Vec3, yaw_degrees: f32) {
        if let Ok(mut m) = self.mixer.lock() {
            m.listener = eye;
            // Yaw 0 looks along +z; the right hand is -x.
            let y = yaw_degrees.to_radians();
            m.right = Vec3::new(-y.cos(), 0.0, -y.sin());
        }
    }

    pub fn set_volume(&self, c: Category, v: f32) {
        if let Ok(mut m) = self.mixer.lock() {
            m.volumes.insert(c, v.clamp(0.0, 1.0));
        }
    }

    /// `SoundManager.playSound` for `PositionedSoundRecord(event, category, volume, pitch, at)`;
    /// `at` None plays it unattenuated, as the menu click and music do.
    pub fn play(&self, event: &str, category: Category, at: Option<Vec3>, volume: f32, pitch: f32) -> Option<SoundId> {
        if self.muted {
            return None;
        }
        let assets = self.assets.as_ref()?;
        let entry = {
            let mut r = self.rand.lock().ok()?;
            assets.pick(event, &mut r, 0)?
        };
        let volume = volume * entry.volume;
        let pitch = (pitch * entry.pitch).clamp(0.5, 2.0);
        let source = if entry.stream {
            let path = assets.path_of(&entry.name)?;
            let reader = OggStreamReader::new(BufReader::new(File::open(path).ok()?)).ok()?;
            let channels = reader.ident_hdr.audio_channels as usize;
            let rate = reader.ident_hdr.audio_sample_rate;
            Source::Stream { reader: Box::new(reader), channels, rate, buf: Vec::new(), base: 0, done: false }
        } else {
            Source::Memory(assets.pcm(&entry.name)?)
        };
        let id = {
            let mut n = self.next_id.lock().ok()?;
            *n += 1;
            *n
        };
        let mut m = self.mixer.lock().ok()?;
        if m.voices.len() >= MAX_VOICES {
            // The oldest positional voice goes first.
            if let Some(i) = m.voices.iter().position(|v| v.at.is_some()) {
                m.voices.remove(i);
            } else {
                return None;
            }
        }
        m.voices.push(Voice { id, source, pos: 0.0, pitch, gain: volume.clamp(0.0, 1.0), category, at, range: 16.0 * volume.max(1.0) });
        Some(SoundId(id))
    }

    /// `SoundHandler.stopSound`: the voice ends at once.
    pub fn stop(&self, id: SoundId) {
        if let Ok(mut m) = self.mixer.lock() {
            m.voices.retain(|v| v.id != id.0);
        }
    }

    pub fn playing(&self, id: SoundId) -> bool {
        self.mixer.lock().is_ok_and(|m| m.voices.iter().any(|v| v.id == id.0))
    }
}

/// The open output device. Dropping it stops the sound.
pub struct OutputStream {
    stream: cpal::Stream,
    target: Arc<Mutex<Option<Arc<Mutex<Mixer>>>>>,
}

// cpal's stream handle is only used to keep the stream alive on the main thread.
unsafe impl Send for OutputStream {}
unsafe impl Sync for OutputStream {}

impl OutputStream {
    fn open() -> Option<(OutputStream, u32)> {
        use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
        let device = cpal::default_host().default_output_device()?;
        let config = device.default_output_config().ok()?;
        let rate = config.sample_rate().0;
        let channels = config.channels() as usize;
        let target: Arc<Mutex<Option<Arc<Mutex<Mixer>>>>> = Arc::new(Mutex::new(None));
        let t = target.clone();
        let mut scratch: Vec<f32> = Vec::new();
        let stream = device
            .build_output_stream(
                &config.config(),
                move |data: &mut [f32], _| {
                    let frames = data.len() / channels.max(1);
                    scratch.clear();
                    scratch.resize(frames * 2, 0.0);
                    if let Some(m) = t.lock().ok().and_then(|g| g.clone()) {
                        if let Ok(mut m) = m.lock() {
                            m.mix(&mut scratch);
                        }
                    }
                    for (f, o) in data.chunks_mut(channels.max(1)).enumerate() {
                        for (c, s) in o.iter_mut().enumerate() {
                            *s = scratch[f * 2 + c.min(1)].clamp(-1.0, 1.0);
                        }
                    }
                },
                |e| eprintln!("audio output: {e}"),
                None,
            )
            .ok()?;
        stream.play().ok()?;
        Some((OutputStream { stream, target }, rate))
    }

    fn attach(&self, mixer: Arc<Mutex<Mixer>>) {
        if let Ok(mut t) = self.target.lock() {
            *t = Some(mixer);
        }
        let _ = &self.stream;
    }
}

/// Keeps the output open for the app's life.
#[derive(Resource)]
pub struct Output(#[allow(dead_code)] pub Option<OutputStream>);

/// `MusicTicker` for the creative music type.
#[derive(Resource)]
pub struct MusicTicker {
    rand: JavaRandom,
    current: Option<SoundId>,
    time_until_next: i32,
}

impl Default for MusicTicker {
    fn default() -> Self {
        MusicTicker { rand: JavaRandom::new(seed() ^ 0x1234_5678), current: None, time_until_next: 100 }
    }
}

/// `MusicType.CREATIVE`
const MUSIC_MIN: i32 = 1200;
const MUSIC_MAX: i32 = 3600;

/// `MathHelper.getInt(rand, min, max)`
fn get_int(r: &mut JavaRandom, min: i32, max: i32) -> i32 {
    if min >= max {
        min
    } else {
        r.next_int_bounded(max - min + 1) + min
    }
}

impl MusicTicker {
    /// `MusicTicker.update`, once per client tick.
    pub fn update(&mut self, audio: &Audio) {
        if let Some(id) = self.current {
            if !audio.playing(id) {
                self.current = None;
                self.time_until_next = get_int(&mut self.rand, MUSIC_MIN, MUSIC_MAX).min(self.time_until_next);
            }
        }
        self.time_until_next = self.time_until_next.min(MUSIC_MAX);
        if self.current.is_none() {
            let t = self.time_until_next;
            self.time_until_next -= 1;
            if t <= 0 {
                self.current = audio.play("music.creative", Category::Music, None, 1.0, 1.0);
                self.time_until_next = i32::MAX;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(rate: u32, frames: usize) -> Arc<Pcm> {
        Arc::new(Pcm { channels: 1, rate, samples: (0..frames).map(|i| if i % 2 == 0 { 16384 } else { -16384 }).collect() })
    }

    fn mixer_with(at: Option<Vec3>, pitch: f32, frames: usize) -> Mixer {
        let mut m = Mixer::new(48_000);
        m.voices.push(Voice { id: 1, source: Source::Memory(tone(48_000, frames)), pos: 0.0, pitch, gain: 1.0, category: Category::Blocks, at, range: 16.0 });
        m
    }

    #[test]
    fn a_voice_ends_with_its_samples_and_pitch_shortens_it() {
        let mut m = mixer_with(None, 1.0, 100);
        let mut out = vec![0f32; 2 * 64];
        m.mix(&mut out);
        assert_eq!(m.voices.len(), 1);
        m.mix(&mut out);
        assert!(m.voices.is_empty(), "100 frames end inside the second 64");
        let mut m = mixer_with(None, 2.0, 100);
        let mut out = vec![0f32; 2 * 64];
        m.mix(&mut out);
        assert!(m.voices.is_empty(), "twice the pitch plays 100 frames in 50");
    }

    #[test]
    fn distance_fades_and_the_side_pans() {
        let loud = |at: Vec3| {
            let mut m = mixer_with(Some(at), 1.0, 1000);
            m.right = Vec3::new(-1.0, 0.0, 0.0);
            let mut out = vec![0f32; 2 * 16];
            m.mix(&mut out);
            let l: f32 = out.iter().step_by(2).map(|s| s.abs()).sum();
            let r: f32 = out.iter().skip(1).step_by(2).map(|s| s.abs()).sum();
            (l, r)
        };
        let (near_l, near_r) = loud(Vec3::new(0.0, 0.0, 2.0));
        let (far_l, _) = loud(Vec3::new(0.0, 0.0, 12.0));
        let (gone_l, gone_r) = loud(Vec3::new(0.0, 0.0, 20.0));
        assert!(near_l > far_l && far_l > 0.0);
        assert!((near_l - near_r).abs() < 1e-3, "straight ahead is centered");
        assert_eq!((gone_l, gone_r), (0.0, 0.0), "past 16 blocks nothing");
        let (side_l, side_r) = loud(Vec3::new(-4.0, 0.0, 0.0));
        // Facing +z, -x is on the right.
        assert!(side_l < side_r * 0.1, "{side_l} {side_r}");
    }

    #[test]
    fn events_parse_with_weights_and_references() {
        let v: Value = serde_json::from_str(
            r#"{"block.stone.break": {"sounds": ["dig/stone1", {"name": "dig/stone2", "weight": 3, "pitch": 0.8}]},
                "music.creative": {"sounds": [{"name": "music.game", "type": "event"}, {"name": "music/game/creative/creative1", "stream": true}]}}"#,
        )
        .unwrap();
        let e = parse_events(&v);
        assert_eq!(e["block.stone.break"].len(), 2);
        assert_eq!(e["block.stone.break"][1].weight, 3);
        assert!((e["block.stone.break"][1].pitch - 0.8).abs() < 1e-6);
        assert!(e["music.creative"][0].event && e["music.creative"][1].stream);
    }

    #[test]
    fn music_waits_100_ticks_first() {
        let audio = Audio::silent();
        let mut t = MusicTicker::default();
        for _ in 0..100 {
            t.update(&audio);
        }
        assert_eq!(t.time_until_next, 0);
        t.update(&audio);
        assert_eq!(t.time_until_next, i32::MAX, "it tried to start the music (silent here)");
    }
}
