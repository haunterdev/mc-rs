//! The streamed session's video: each frame read back from the GPU goes to an `ffmpeg` child as
//! raw BGRA, and the H.264 it writes comes back split into access units for the server.
//!
//! The encoder runs with no B-frames, one frame in flight and an access unit delimiter in front of
//! every picture. An access unit ends where the next delimiter starts, or when `ffmpeg` has been
//! quiet for a millisecond after a picture, which is the usual case: it writes a whole access
//! unit at once and the next one only after the next frame comes in.

use std::io::{self, BufRead, BufReader, Read, Write};
use std::process::{Child, Command, Stdio};
use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use crossbeam_channel::{bounded, unbounded, Sender, TryRecvError, TrySendError};

/// How long ffmpeg's output has to pause before the gathered bytes count as a whole access unit;
/// it writes a unit in pieces of up to 32 KB back to back, microseconds apart.
const QUIET: Duration = Duration::from_millis(1);

/// What the server asked for on the command line.
#[derive(Clone, Debug)]
pub struct VideoConfig {
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    /// An ffmpeg H.264 encoder: `libx264` (the CPU, the default), `h264_amf`, `h264_nvenc` or `h264_qsv`.
    pub encoder: String,
    pub bitrate_kbps: u32,
}

/// The ffmpeg arguments for `cfg`, input on stdin and output on stdout.
pub fn ffmpeg_args(cfg: &VideoConfig) -> Vec<String> {
    let gop = (cfg.fps * 2).max(30);
    let br = cfg.bitrate_kbps.max(500);
    let mut a: Vec<String> = [
        "-hide_banner",
        "-loglevel",
        "error",
        "-f",
        "rawvideo",
        "-pix_fmt",
        "bgra",
        "-video_size",
        &format!("{}x{}", cfg.width, cfg.height),
        "-framerate",
        &cfg.fps.to_string(),
        "-i",
        "pipe:0",
        // Frames go out as they come in: no duplicating or dropping to hold a constant rate.
        "-fps_mode",
        "passthrough",
        "-an",
        "-c:v",
        &cfg.encoder,
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    let rate = [
        "-b:v".to_string(),
        format!("{br}k"),
        "-maxrate".into(),
        format!("{br}k"),
        // A rate buffer of two frames: a key frame cannot burst past what the link carries in
        // that time, which would queue every frame behind it.
        "-bufsize".into(),
        format!("{}k", (br * 2 / cfg.fps.max(1)).max(100)),
        "-g".into(),
        gop.to_string(),
        "-bf".into(),
        "0".into(),
    ];
    a.extend(rate);
    let extra: Vec<String> = match cfg.encoder.as_str() {
        "h264_amf" => vec![
            // Constrained baseline has no frame reordering, so a browser's decoder hands each
            // picture out as soon as it decodes it instead of holding a reorder window.
            "-usage", "ultralowlatency", "-quality", "speed", "-rc", "cbr", "-aud", "1", "-async_depth", "1", "-profile:v",
            "constrained_baseline", "-preanalysis", "0", "-enforce_hrd", "1",
        ]
        .into_iter()
        .map(String::from)
        // AMF's low-latency usage ignores -g; ffmpeg forces an I frame every two seconds, and
        // forced_idr makes it an IDR a browser can start from after dropping frames.
        .chain(["-header_spacing".to_string(), gop.to_string(), "-force_key_frames".into(), "expr:gte(t,n_forced*2)".into(), "-forced_idr".into(), "1".into()])
        .collect(),
        "h264_nvenc" => ["-preset", "p1", "-tune", "ull", "-rc", "cbr", "-aud", "1", "-zerolatency", "1", "-delay", "0", "-profile:v", "baseline", "-pix_fmt", "yuv420p"]
            .into_iter()
            .map(String::from)
            .collect(),
        "h264_qsv" => ["-preset", "veryfast", "-low_power", "1", "-aud", "1", "-async_depth", "1", "-profile:v", "baseline", "-pix_fmt", "nv12"]
            .into_iter()
            .map(String::from)
            .collect(),
        // The default (libx264 on the CPU): zerolatency has no lookahead and no frame threads, so
        // each frame comes out in 2 to 3 ms at 1280x720 (tools/encoder_latency.py); the GPU
        // encoders behind ffmpeg hold every frame until the next one comes in (h264_amf does,
        // 18 ms at 60 fps), since ffmpeg asks them for output only when it gives them input.
        // Four slice threads keep a session to about half a core at 1280x720, 60 fps.
        _ => ["-preset", "superfast", "-tune", "zerolatency", "-threads", "4", "-profile:v", "baseline", "-pix_fmt", "yuv420p", "-x264-params", "aud=1:repeat-headers=1"]
            .into_iter()
            .map(String::from)
            .collect(),
    };
    a.extend(extra);
    a.extend(["-flush_packets", "1", "-f", "h264", "pipe:1"].map(String::from));
    a
}

/// One frame for the encoder: tightly packed BGRA rows, the newest browser input the frame
/// reflects (`proto::Input::Seq`), and when the frame's update began.
pub struct Frame {
    pub data: Vec<u8>,
    pub seq: u32,
    pub at: Instant,
}

/// An encoded access unit and its frame's input sequence and time from update to here.
pub struct Unit {
    pub key: bool,
    pub bytes: Vec<u8>,
    pub seq: u32,
    pub pipeline_us: u32,
    /// From the frame written to ffmpeg to the unit's first byte back, and to the unit leaving.
    pub first_byte_us: u32,
    pub encode_us: u32,
}

pub struct Encoder {
    frames: Sender<Frame>,
    pub dropped: Arc<AtomicU64>,
    child: Child,
}

impl Drop for Encoder {
    fn drop(&mut self) {
        let _ = self.child.kill();
    }
}

impl Encoder {
    /// Starts ffmpeg; each finished access unit goes to `out`.
    pub fn start(cfg: &VideoConfig, out: Sender<Unit>) -> io::Result<Encoder> {
        let exe = std::env::var("MCRS_FFMPEG").unwrap_or_else(|_| "ffmpeg".into());
        let mut cmd = Command::new(exe);
        cmd.args(ffmpeg_args(cfg)).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            // CREATE_NO_WINDOW
            cmd.creation_flags(0x0800_0000);
        }
        let mut child = cmd.spawn()?;
        let mut stdin = child.stdin.take().expect("piped stdin");
        let mut stdout = child.stdout.take().expect("piped stdout");
        let stderr = child.stderr.take().expect("piped stderr");
        // One frame in hand at most: a frame that waits behind another is a frame of lag.
        let (frames, frame_rx) = bounded::<Frame>(1);
        // Per frame in flight: its sequence, update start, and when ffmpeg had all of it.
        let pending: Arc<Mutex<VecDeque<(u32, Instant, Instant)>>> = Arc::new(Mutex::new(VecDeque::new()));
        let p = pending.clone();
        thread::Builder::new().name("encode-in".into()).spawn(move || {
            for f in frame_rx {
                if stdin.write_all(&f.data).and_then(|_| stdin.flush()).is_err() {
                    return;
                }
                if let Ok(mut q) = p.lock() {
                    q.push_back((f.seq, f.at, Instant::now()));
                }
            }
        })?;
        let (chunk_tx, chunk_rx) = unbounded::<(Instant, Vec<u8>)>();
        thread::Builder::new().name("encode-out".into()).spawn(move || {
            let mut buf = vec![0u8; 1 << 16];
            loop {
                match stdout.read(&mut buf) {
                    Ok(0) | Err(_) => return,
                    Ok(n) => {
                        if chunk_tx.send((Instant::now(), buf[..n].to_vec())).is_err() {
                            return;
                        }
                    }
                }
            }
        })?;
        thread::Builder::new().name("encode-split".into()).spawn(move || {
            let mut au = Splitter::default();
            let us = |d: Duration| d.as_micros().min(u32::MAX as u128) as u32;
            // When the oldest unit still being gathered got its first byte.
            let mut first_byte: Option<Instant> = None;
            // Access units leave in the order their frames went in.
            let send = |(key, bytes): (bool, Vec<u8>), first: Option<Instant>| {
                let now = Instant::now();
                let (seq, at, written) = pending.lock().ok().and_then(|mut q| q.pop_front()).unwrap_or((0, now, now));
                let first_byte_us = first.map_or(0, |f| us(f.saturating_duration_since(written)));
                out.send(Unit { key, bytes, seq, pipeline_us: us(at.elapsed()), first_byte_us, encode_us: us(written.elapsed()) }).is_ok()
            };
            // When the newest chunk came, while a unit is being gathered.
            let mut gathering: Option<Instant> = None;
            loop {
                // Idle, block until ffmpeg writes. While gathering, poll with short sleeps until
                // it has been quiet for QUIET: a timed channel wait on Windows parks until the
                // next 15.6 ms timer tick (8 ms late on average, measured), where std's sleep
                // uses a high-resolution timer.
                let got = match gathering {
                    None => match chunk_rx.recv() {
                        Ok(c) => Some(c),
                        Err(_) => return,
                    },
                    Some(last) => match chunk_rx.try_recv() {
                        Ok(c) => Some(c),
                        Err(TryRecvError::Disconnected) => return,
                        Err(TryRecvError::Empty) if last.elapsed() >= QUIET => None,
                        Err(TryRecvError::Empty) => {
                            thread::sleep(Duration::from_micros(100));
                            continue;
                        }
                    },
                };
                match got {
                    Some((t, c)) => {
                        gathering = Some(t);
                        first_byte.get_or_insert(t);
                        let units = au.push(&c);
                        let n = units.len();
                        for (i, unit) in units.into_iter().enumerate() {
                            // A unit closed by the next one's delimiter: that next unit started
                            // in this chunk.
                            let f = first_byte.take();
                            if i + 1 == n && au.has_pending() {
                                first_byte = Some(t);
                            }
                            if !send(unit, f) {
                                return;
                            }
                        }
                    }
                    None => {
                        gathering = None;
                        if let Some(unit) = au.flush() {
                            if !send(unit, first_byte.take()) {
                                return;
                            }
                        }
                    }
                }
            }
        })?;
        thread::Builder::new().name("encode-log".into()).spawn(move || {
            for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                eprintln!("ffmpeg: {line}");
            }
        })?;
        Ok(Encoder { frames, dropped: Arc::new(AtomicU64::new(0)), child })
    }

    /// Where the frame grab (grab.rs) hands frames; a frame that finds the encoder busy is
    /// dropped and counted.
    pub fn sink(&self) -> FrameSink {
        FrameSink { frames: self.frames.clone(), dropped: self.dropped.clone() }
    }

    pub fn alive(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(None))
    }
}

#[derive(Clone)]
pub struct FrameSink {
    frames: Sender<Frame>,
    dropped: Arc<AtomicU64>,
}

impl FrameSink {
    pub fn submit(&self, f: Frame) {
        if let Err(TrySendError::Full(_)) = self.frames.try_send(f) {
            self.dropped.fetch_add(1, Ordering::Relaxed);
        }
    }
}

/// Gathers ffmpeg's output into access units.
#[derive(Default)]
pub struct Splitter {
    pending: Vec<u8>,
}

fn is_vcl(t: u8) -> bool {
    t == 1 || t == 5
}

fn finish(bytes: Vec<u8>) -> (bool, Vec<u8>) {
    let key = proto::nal_units(&bytes).iter().any(|n| proto::nal_type(n) == 5);
    (key, bytes)
}

impl Splitter {
    /// Adds output; returns every access unit a following delimiter closed.
    pub fn push(&mut self, data: &[u8]) -> Vec<(bool, Vec<u8>)> {
        self.pending.extend_from_slice(data);
        let mut out = Vec::new();
        loop {
            // A delimiter (type 9) after a picture starts the next access unit.
            let p = &self.pending;
            let mut cut = None;
            let mut seen_vcl = false;
            let mut i = 0;
            while i + 3 < p.len() {
                if p[i] == 0 && p[i + 1] == 0 && p[i + 2] == 1 {
                    let t = p[i + 3] & 0x1F;
                    if t == 9 && seen_vcl {
                        // Keep a four-byte start code's leading zero with the next unit.
                        cut = Some(if i > 0 && p[i - 1] == 0 { i - 1 } else { i });
                        break;
                    }
                    seen_vcl |= is_vcl(t);
                    i += 3;
                } else {
                    i += 1;
                }
            }
            let Some(c) = cut else { break };
            let rest = self.pending.split_off(c);
            out.push(finish(std::mem::replace(&mut self.pending, rest)));
        }
        out
    }

    pub fn has_pending(&self) -> bool {
        !self.pending.is_empty()
    }

    /// The pending bytes as an access unit once they hold a picture and the encoder went quiet.
    pub fn flush(&mut self) -> Option<(bool, Vec<u8>)> {
        let has_vcl = proto::nal_units(&self.pending).iter().any(|n| is_vcl(proto::nal_type(n)));
        has_vcl.then(|| finish(std::mem::take(&mut self.pending)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const AUD: [u8; 6] = [0, 0, 0, 1, 9, 0xF0];
    const SPS: [u8; 6] = [0, 0, 0, 1, 0x67, 1];
    const IDR: [u8; 6] = [0, 0, 0, 1, 0x65, 2];
    const P: [u8; 6] = [0, 0, 0, 1, 0x41, 3];

    #[test]
    fn delimiters_split_access_units() {
        let mut s = Splitter::default();
        let first: Vec<u8> = [AUD, SPS, IDR].concat();
        assert!(s.push(&first).is_empty(), "the unit is open until something closes it");
        let units = s.push(&[AUD, P].concat());
        assert_eq!(units, vec![(true, first)]);
        assert_eq!(s.flush(), Some((false, [AUD, P].concat())));
        assert_eq!(s.flush(), None);
    }

    #[test]
    fn a_unit_split_across_reads_waits_for_its_picture() {
        let mut s = Splitter::default();
        assert!(s.push(&[AUD, SPS].concat()).is_empty());
        assert_eq!(s.flush(), None, "no picture yet");
        assert!(s.push(&IDR).is_empty());
        assert_eq!(s.flush().map(|u| u.0), Some(true));
    }

    #[test]
    fn amf_arguments_keep_latency_low() {
        let cfg = VideoConfig { width: 1280, height: 720, fps: 60, encoder: "h264_amf".into(), bitrate_kbps: 8000 };
        let a = ffmpeg_args(&cfg).join(" ");
        assert!(a.contains("-video_size 1280x720"));
        assert!(a.contains("-bf 0") && a.contains("-async_depth 1") && a.contains("-aud 1"));
        assert!(a.contains("-force_key_frames expr:gte(t,n_forced*2) -forced_idr 1"));
        assert!(a.contains("-profile:v constrained_baseline") && a.contains("-bufsize 266k") && a.contains("-fps_mode passthrough"));
        assert!(a.ends_with("-f h264 pipe:1"));
    }

    #[test]
    fn x264_arguments_hand_frames_straight_back() {
        let cfg = VideoConfig { width: 1280, height: 720, fps: 60, encoder: "libx264".into(), bitrate_kbps: 6000 };
        let a = ffmpeg_args(&cfg).join(" ");
        assert!(a.contains("-tune zerolatency") && a.contains("-preset superfast") && a.contains("-threads 4"));
        assert!(a.contains("-g 120") && a.contains("aud=1:repeat-headers=1") && a.contains("-pix_fmt yuv420p"));
        assert!(a.ends_with("-flush_packets 1 -f h264 pipe:1"));
    }
}
