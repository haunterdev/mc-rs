//! Messages between the mc-rs server (crates/server) and the viewer sessions it starts, one per
//! browser player, plus the input events the browser page sends.
//!
//! Server and viewer talk over one local TCP connection in frames: a little-endian `u32` length
//! of what follows, a tag byte, then the body. Viewer to server tags are below 100, server to
//! viewer tags are 100 and up. Bodies are fixed binary layouts, all little-endian, except
//! [`S_WELCOME`], which is JSON.
//!
//! A native client (the launcher's Join, `viewer --join`) renders on its own PC and speaks the
//! same frames over a WebSocket on the server's web port (crates/server/src/native.rs): each
//! binary message holds whole frames, and the page's JSON travels as [`V_TEXT`] and [`S_TEXT`].

use std::io::{self, Read, Write};

mod survival; pub use survival::*;

/// The viewer's first frame: the session token the server gave it on the command line (UTF-8).
pub const V_HELLO: u8 = 1;
/// One encoded H.264 access unit: [`VideoHeader`] then the Annex B bytes.
pub const V_VIDEO: u8 = 2;
/// Interleaved stereo `i16` PCM at [`AUDIO_RATE`].
pub const V_AUDIO: u8 = 3;
/// The session player's [`PlayerState`], every client tick.
pub const V_STATE: u8 = 4;
/// Blocks the session player's edits changed: [`encode_blocks`].
pub const V_BLOCKS: u8 = 5;
/// A line for the server log (UTF-8). Lines starting `stats ` also go to `latency.log`.
pub const V_LOG: u8 = 6;
/// Who has the mouse, one byte: [`FOCUS_NONE`], [`FOCUS_GAME`] or [`FOCUS_MENU`].
pub const V_FOCUS: u8 = 7;

/// A media file for a painting: [`encode_media`]. Sent before the first [`V_PAINTING`] that
/// shows it.
pub const V_MEDIA: u8 = 8;
/// A painting hung, resized or taken down: [`Painting`].
pub const V_PAINTING: u8 = 9;

/// Not captured and no screen open: a click into the picture captures the mouse.
pub const FOCUS_NONE: u8 = 0;
/// The game holds the mouse: the page locks the pointer.
pub const FOCUS_GAME: u8 = 1;
/// A screen is open: the page shows the free cursor and reports where it is.
pub const FOCUS_MENU: u8 = 2;

/// A native client's JSON message, as the browser page would send it (`{"t":"chat","text":"hi"}`).
pub const V_TEXT: u8 = 40;
/// To a native client: one of the JSON messages the browser page gets (`chat`, `players`,
/// `closed` and the rest).
pub const S_TEXT: u8 = 140;

/// JSON: `{"id":1,"name":"xy","pos":[x,y,z],"yaw":0,"pitch":0,"flying":false}`.
pub const S_WELCOME: u8 = 101;
/// Browser input events as the page sent them: [`decode_inputs`].
pub const S_INPUT: u8 = 102;
/// Another player joined: `u32` id, then the name (UTF-8).
pub const S_JOIN: u8 = 103;
/// A player left: `u32` id.
pub const S_LEAVE: u8 = 104;
/// Another player's state: `u32` id, then a [`PlayerState`].
pub const S_STATE: u8 = 105;
/// Blocks another player changed, or the saved world's edits on join: [`encode_blocks`].
pub const S_BLOCKS: u8 = 106;
/// A media file another player shared, or one the saved paintings show: [`encode_media`].
pub const S_MEDIA: u8 = 107;
/// A painting another player hung, resized or took down, or a saved one on join: [`Painting`].
pub const S_PAINTING: u8 = 108;
/// The session player uploaded this media id from the page: hang it where the crosshair is.
pub const S_HANG: u8 = 109;
/// The browser's tuner changed the streamed picture mid-session: [`encode_picture`].
pub const S_PICTURE: u8 = 110;

/// Width, height, frames a second and bitrate in kbit/s, each a little-endian u32.
pub fn encode_picture(p: [u32; 4]) -> Vec<u8> {
    p.iter().flat_map(|v| v.to_le_bytes()).collect()
}

pub fn decode_picture(b: &[u8]) -> Option<[u32; 4]> {
    let v = |i: usize| Some(u32::from_le_bytes(b.get(i * 4..i * 4 + 4)?.try_into().ok()?));
    Some([v(0)?, v(1)?, v(2)?, v(3)?])
}

/// The shared clock: [`TimeSync`]. Sent on join, every second, and at once after `/time`.
pub const S_TIME: u8 = 120;
/// Moves the session's own player: [`Teleport`].
pub const S_TELEPORT: u8 = 121;
/// The session player's game mode, one byte: [`GM_SURVIVAL`] to [`GM_SPECTATOR`].
pub const S_GAMEMODE: u8 = 122;

/// `GameType` ids, as `/gamemode 0..3` names them.
pub const GM_SURVIVAL: u8 = 0;
pub const GM_CREATIVE: u8 = 1;
pub const GM_ADVENTURE: u8 = 2;
pub const GM_SPECTATOR: u8 = 3;

/// The server's `WorldInfo.worldTime` and whether `doDaylightCycle` advances it.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct TimeSync {
    pub world_time: i64,
    pub cycle: bool,
    /// Jump straight to the time (a `/time` command) instead of easing toward it.
    pub snap: bool,
}

impl TimeSync {
    pub const SIZE: usize = 9;

    pub fn encode(&self) -> [u8; Self::SIZE] {
        let mut b = [0u8; Self::SIZE];
        b[..8].copy_from_slice(&self.world_time.to_le_bytes());
        b[8] = self.cycle as u8 | (self.snap as u8) << 1;
        b
    }

    pub fn decode(b: &[u8]) -> Option<TimeSync> {
        (b.len() >= Self::SIZE).then(|| TimeSync {
            world_time: i64::from_le_bytes(b[..8].try_into().unwrap()),
            cycle: b[8] & 1 != 0,
            snap: b[8] & 2 != 0,
        })
    }
}

/// [`Teleport`] flags `SPacketPlayerPosLook.EnumFlags`: the value is added to the player's own
/// x, y, z, yaw or pitch. Motion along an axis that is set outright stops.
pub const TP_REL_X: u8 = 1;
pub const TP_REL_Y: u8 = 2;
pub const TP_REL_Z: u8 = 4;
pub const TP_REL_YAW: u8 = 8;
pub const TP_REL_PITCH: u8 = 16;
/// [`Teleport`] flag: ignore y and stand on the column's top block that blocks movement
/// (`World.getTopSolidOrLiquidBlock`), as a respawn at the world spawn does.
pub const TP_SURFACE: u8 = 32;
/// [`Teleport`] flag: a respawn: motion and flying end, and the player rises out of any blocks.
pub const TP_RESPAWN: u8 = 64;

/// Where the server puts the session's player.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Teleport {
    pub pos: [f64; 3],
    pub yaw: f32,
    pub pitch: f32,
    pub flags: u8,
}

impl Teleport {
    pub const SIZE: usize = 33;

    pub fn encode(&self) -> [u8; Self::SIZE] {
        let mut b = [0u8; Self::SIZE];
        for i in 0..3 {
            b[i * 8..i * 8 + 8].copy_from_slice(&self.pos[i].to_le_bytes());
        }
        b[24..28].copy_from_slice(&self.yaw.to_le_bytes());
        b[28..32].copy_from_slice(&self.pitch.to_le_bytes());
        b[32] = self.flags;
        b
    }

    pub fn decode(b: &[u8]) -> Option<Teleport> {
        if b.len() < Self::SIZE {
            return None;
        }
        let f64_at = |i: usize| f64::from_le_bytes(b[i..i + 8].try_into().unwrap());
        let f32_at = |i: usize| f32::from_le_bytes(b[i..i + 4].try_into().unwrap());
        let t = Teleport { pos: [f64_at(0), f64_at(8), f64_at(16)], yaw: f32_at(24), pitch: f32_at(28), flags: b[32] };
        (t.pos.iter().all(|v| v.is_finite()) && t.yaw.is_finite() && t.pitch.is_finite()).then_some(t)
    }
}

/// [`V_VIDEO`] flag: the access unit holds an IDR picture.
pub const VIDEO_KEY: u8 = 1;

/// What precedes each access unit: flags, the newest browser input sequence ([`Input::Seq`]) the
/// picture reflects, and the microseconds from the start of the frame's update to the encoded
/// unit (render, capture and encode on the host).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct VideoHeader {
    pub flags: u8,
    pub seq: u32,
    pub pipeline_us: u32,
}

impl VideoHeader {
    pub const SIZE: usize = 9;

    pub fn encode(&self) -> [u8; Self::SIZE] {
        let mut b = [0u8; Self::SIZE];
        b[0] = self.flags;
        b[1..5].copy_from_slice(&self.seq.to_le_bytes());
        b[5..9].copy_from_slice(&self.pipeline_us.to_le_bytes());
        b
    }

    pub fn decode(b: &[u8]) -> Option<VideoHeader> {
        (b.len() >= Self::SIZE).then(|| VideoHeader {
            flags: b[0],
            seq: u32::from_le_bytes(b[1..5].try_into().unwrap()),
            pipeline_us: u32::from_le_bytes(b[5..9].try_into().unwrap()),
        })
    }
}

pub const AUDIO_RATE: u32 = 48_000;

/// Frames past this size are refused as corrupt.
pub const MAX_FRAME: usize = 16 << 20;

pub fn write_frame(w: &mut impl Write, tag: u8, body: &[u8]) -> io::Result<()> {
    let len = (body.len() + 1) as u32;
    let mut head = [0u8; 5];
    head[..4].copy_from_slice(&len.to_le_bytes());
    head[4] = tag;
    w.write_all(&head)?;
    w.write_all(body)
}

/// One frame as bytes, for writers that queue them.
pub fn frame(tag: u8, body: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(body.len() + 5);
    write_frame(&mut out, tag, body).expect("writing to a Vec");
    out
}

pub fn read_frame(r: &mut impl Read) -> io::Result<(u8, Vec<u8>)> {
    let mut len = [0u8; 4];
    r.read_exact(&mut len)?;
    let len = u32::from_le_bytes(len) as usize;
    if len == 0 || len > MAX_FRAME {
        return Err(io::Error::new(io::ErrorKind::InvalidData, format!("frame length {len}")));
    }
    let mut body = vec![0u8; len];
    r.read_exact(&mut body)?;
    let tag = body.remove(0);
    Ok((tag, body))
}

/// The whole frames one native client message holds, as tag and body; `None` when the message
/// ends inside a frame or holds a corrupt length.
pub fn split_frames(mut b: &[u8]) -> Option<Vec<(u8, &[u8])>> {
    let mut out = Vec::new();
    while !b.is_empty() {
        let len = u32::from_le_bytes(b.get(..4)?.try_into().unwrap()) as usize;
        if len == 0 || len > MAX_FRAME {
            return None;
        }
        let f = b.get(4..4 + len)?;
        out.push((f[0], &f[1..]));
        b = &b[4 + len..];
    }
    Some(out)
}

pub const ON_GROUND: u8 = 1;
pub const SNEAKING: u8 = 2;
pub const SPRINTING: u8 = 4;
pub const FLYING: u8 = 8;

/// What other players need to draw a player.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PlayerState {
    /// Feet position.
    pub pos: [f64; 3],
    pub yaw: f32,
    pub pitch: f32,
    pub flags: u8,
    /// Counts arm swings (wrapping), so a receiver starts one per step.
    pub swings: u8,
    /// The held block state (`id << 4 | meta`), 0 for an empty hand.
    pub held: u16,
}

impl PlayerState {
    pub const SIZE: usize = 36;

    pub fn encode(&self) -> [u8; Self::SIZE] {
        let mut b = [0u8; Self::SIZE];
        for i in 0..3 {
            b[i * 8..i * 8 + 8].copy_from_slice(&self.pos[i].to_le_bytes());
        }
        b[24..28].copy_from_slice(&self.yaw.to_le_bytes());
        b[28..32].copy_from_slice(&self.pitch.to_le_bytes());
        b[32] = self.flags;
        b[33] = self.swings;
        b[34..36].copy_from_slice(&self.held.to_le_bytes());
        b
    }

    pub fn decode(b: &[u8]) -> Option<PlayerState> {
        if b.len() < Self::SIZE {
            return None;
        }
        let f64_at = |i: usize| f64::from_le_bytes(b[i..i + 8].try_into().unwrap());
        let f32_at = |i: usize| f32::from_le_bytes(b[i..i + 4].try_into().unwrap());
        let s = PlayerState {
            pos: [f64_at(0), f64_at(8), f64_at(16)],
            yaw: f32_at(24),
            pitch: f32_at(28),
            flags: b[32],
            swings: b[33],
            held: u16::from_le_bytes([b[34], b[35]]),
        };
        (s.pos.iter().all(|v| v.is_finite()) && s.yaw.is_finite() && s.pitch.is_finite()).then_some(s)
    }
}

/// One changed block: position and new state.
pub type BlockChange = ((i32, i32, i32), u16);

/// 14 bytes a block: `i32` x, y, z, `u16` state.
pub fn encode_blocks(changes: &[BlockChange]) -> Vec<u8> {
    let mut out = Vec::with_capacity(changes.len() * 14);
    for &((x, y, z), s) in changes {
        out.extend_from_slice(&x.to_le_bytes());
        out.extend_from_slice(&y.to_le_bytes());
        out.extend_from_slice(&z.to_le_bytes());
        out.extend_from_slice(&s.to_le_bytes());
    }
    out
}

pub fn decode_blocks(b: &[u8]) -> Vec<BlockChange> {
    b.chunks_exact(14)
        .map(|c| {
            let i = |k: usize| i32::from_le_bytes(c[k..k + 4].try_into().unwrap());
            ((i(0), i(4), i(8)), u16::from_le_bytes([c[12], c[13]]))
        })
        .collect()
}

/// The largest media file a painting may share, so it fits one frame.
pub const MAX_MEDIA: usize = MAX_FRAME - 1024;

/// A media file's name: its FNV-1a 64 hash in hex and its lowercase extension, so the same file
/// is one name everywhere and a name cannot climb out of a directory.
pub fn media_id(bytes: &[u8], ext: &str) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in bytes {
        h = (h ^ b as u64).wrapping_mul(0x0100_0000_01b3);
    }
    let ext: String = ext.chars().filter(|c| c.is_ascii_alphanumeric()).take(8).collect::<String>().to_ascii_lowercase();
    format!("{h:016x}.{ext}")
}

/// Whether `id` has the [`media_id`] shape.
pub fn valid_media_id(id: &str) -> bool {
    let Some((h, ext)) = id.split_once('.') else { return false };
    h.len() == 16 && h.bytes().all(|b| b.is_ascii_hexdigit()) && (1..=8).contains(&ext.len()) && ext.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
}

/// [`V_MEDIA`] and [`S_MEDIA`]: a `u8` id length, the id, then the file.
pub fn encode_media(id: &str, bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(1 + id.len() + bytes.len());
    out.push(id.len() as u8);
    out.extend_from_slice(id.as_bytes());
    out.extend_from_slice(bytes);
    out
}

pub fn decode_media(b: &[u8]) -> Option<(String, &[u8])> {
    let n = *b.first()? as usize;
    let id = std::str::from_utf8(b.get(1..1 + n)?).ok()?;
    valid_media_id(id).then(|| (id.to_string(), &b[1 + n..]))
}

/// A painting on a block face (the viewer's media.rs). `face` and `up` are `Facing` indices
/// (down, up, north, south, west, east); `gone` takes the painting `uid` down.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Painting {
    pub uid: u64,
    pub pos: [i32; 3],
    pub face: u8,
    pub up: u8,
    pub w: u8,
    pub h: u8,
    pub gone: bool,
    pub media: String,
}

impl Painting {
    pub fn encode(&self) -> Vec<u8> {
        let mut b = Vec::with_capacity(25 + self.media.len());
        b.extend_from_slice(&self.uid.to_le_bytes());
        for v in self.pos {
            b.extend_from_slice(&v.to_le_bytes());
        }
        b.extend_from_slice(&[self.face, self.up, self.w, self.h, self.gone as u8]);
        b.extend_from_slice(self.media.as_bytes());
        b
    }

    pub fn decode(b: &[u8]) -> Option<Painting> {
        if b.len() < 25 {
            return None;
        }
        let i = |k: usize| i32::from_le_bytes(b[k..k + 4].try_into().unwrap());
        let p = Painting {
            uid: u64::from_le_bytes(b[..8].try_into().unwrap()),
            pos: [i(8), i(12), i(16)],
            face: b[20],
            up: b[21],
            w: b[22],
            h: b[23],
            gone: b[24] != 0,
            media: String::from_utf8(b[25..].to_vec()).ok()?,
        };
        let ok = p.gone || (p.face < 6 && p.up < 6 && (1..=16).contains(&p.w) && (1..=16).contains(&p.h) && valid_media_id(&p.media));
        ok.then_some(p)
    }
}

/// A browser input event. Key codes are `KeyboardEvent.code` strings ("KeyW", "Space").
#[derive(Clone, Debug, PartialEq)]
pub enum Input {
    KeyDown(String),
    KeyUp(String),
    /// Pointer-locked mouse movement in pixels, y down.
    MouseMove(f32, f32),
    /// `MouseEvent.button`: 0 left, 1 middle, 2 right.
    MouseDown(u8),
    MouseUp(u8),
    /// Wheel lines, positive away from the user.
    Wheel(f32),
    /// The free cursor over the picture, 0 to 1 across and down.
    Cursor(f32, f32),
    /// The page lost focus: every key and button is up.
    ReleaseAll,
    /// The page's running count of input batches, last in each batch; frames carry back the
    /// newest one they reflect, which is how the page measures input to picture.
    Seq(u32),
}

const I_KEY_DOWN: u8 = 1;
const I_KEY_UP: u8 = 2;
const I_MOVE: u8 = 3;
const I_DOWN: u8 = 4;
const I_UP: u8 = 5;
const I_WHEEL: u8 = 6;
const I_CURSOR: u8 = 7;
const I_RELEASE: u8 = 8;
const I_SEQ: u8 = 9;

pub fn encode_inputs(events: &[Input]) -> Vec<u8> {
    let mut out = Vec::new();
    for e in events {
        match e {
            Input::KeyDown(c) | Input::KeyUp(c) => {
                out.push(if matches!(e, Input::KeyDown(_)) { I_KEY_DOWN } else { I_KEY_UP });
                let c = &c.as_bytes()[..c.len().min(255)];
                out.push(c.len() as u8);
                out.extend_from_slice(c);
            }
            Input::MouseMove(x, y) | Input::Cursor(x, y) => {
                out.push(if matches!(e, Input::MouseMove(..)) { I_MOVE } else { I_CURSOR });
                out.extend_from_slice(&x.to_le_bytes());
                out.extend_from_slice(&y.to_le_bytes());
            }
            Input::MouseDown(b) => out.extend_from_slice(&[I_DOWN, *b]),
            Input::MouseUp(b) => out.extend_from_slice(&[I_UP, *b]),
            Input::Wheel(d) => {
                out.push(I_WHEEL);
                out.extend_from_slice(&d.to_le_bytes());
            }
            Input::ReleaseAll => out.push(I_RELEASE),
            Input::Seq(n) => {
                out.push(I_SEQ);
                out.extend_from_slice(&n.to_le_bytes());
            }
        }
    }
    out
}

/// Decodes what the page sent, stopping at the first malformed event.
pub fn decode_inputs(mut b: &[u8]) -> Vec<Input> {
    let mut out = Vec::new();
    let f32_at = |b: &[u8], i: usize| -> Option<f32> { Some(f32::from_le_bytes(b.get(i..i + 4)?.try_into().ok()?)).filter(|v| v.is_finite()) };
    while let Some((&tag, rest)) = b.split_first() {
        let (event, used) = match tag {
            I_KEY_DOWN | I_KEY_UP => {
                let Some(&n) = rest.first() else { break };
                let Some(code) = rest.get(1..1 + n as usize).and_then(|c| std::str::from_utf8(c).ok()) else { break };
                let code = code.to_string();
                (if tag == I_KEY_DOWN { Input::KeyDown(code) } else { Input::KeyUp(code) }, 1 + n as usize)
            }
            I_MOVE | I_CURSOR => {
                let (Some(x), Some(y)) = (f32_at(rest, 0), f32_at(rest, 4)) else { break };
                (if tag == I_MOVE { Input::MouseMove(x, y) } else { Input::Cursor(x, y) }, 8)
            }
            I_DOWN | I_UP => {
                let Some(&btn) = rest.first() else { break };
                (if tag == I_DOWN { Input::MouseDown(btn) } else { Input::MouseUp(btn) }, 1)
            }
            I_WHEEL => {
                let Some(d) = f32_at(rest, 0) else { break };
                (Input::Wheel(d), 4)
            }
            I_RELEASE => (Input::ReleaseAll, 0),
            I_SEQ => {
                let Some(n) = rest.get(0..4) else { break };
                (Input::Seq(u32::from_le_bytes(n.try_into().unwrap())), 4)
            }
            _ => break,
        };
        out.push(event);
        b = &rest[used..];
    }
    out
}

/// Splits an Annex B byte stream into NAL units (without start codes).
pub fn nal_units(stream: &[u8]) -> Vec<&[u8]> {
    let mut starts = Vec::new();
    let mut i = 0;
    while i + 3 <= stream.len() {
        if stream[i] == 0 && stream[i + 1] == 0 && stream[i + 2] == 1 {
            starts.push(i + 3);
            i += 3;
        } else {
            i += 1;
        }
    }
    let mut out = Vec::with_capacity(starts.len());
    for (k, &s) in starts.iter().enumerate() {
        let mut e = starts.get(k + 1).map_or(stream.len(), |&n| n - 3);
        // A four-byte start code leaves its leading zero on the previous unit.
        while e > s && stream[e - 1] == 0 && k + 1 < starts.len() {
            e -= 1;
        }
        out.push(&stream[s..e]);
    }
    out
}

/// The H.264 NAL unit type of a unit from [`nal_units`].
pub fn nal_type(nal: &[u8]) -> u8 {
    nal.first().map_or(0, |b| b & 0x1F)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames_round_trip() {
        let mut buf = Vec::new();
        write_frame(&mut buf, V_LOG, b"hello").unwrap();
        write_frame(&mut buf, S_LEAVE, &7u32.to_le_bytes()).unwrap();
        let mut r = &buf[..];
        assert_eq!(read_frame(&mut r).unwrap(), (V_LOG, b"hello".to_vec()));
        assert_eq!(read_frame(&mut r).unwrap(), (S_LEAVE, 7u32.to_le_bytes().to_vec()));
        assert!(read_frame(&mut r).is_err());
    }

    #[test]
    fn pictures_round_trip() {
        let p = [960, 540, 60, 4500];
        assert_eq!(decode_picture(&encode_picture(p)), Some(p));
        assert_eq!(decode_picture(&encode_picture(p)[..15]), None);
    }

    #[test]
    fn paintings_and_media_round_trip() {
        let id = media_id(b"hello", "PNG");
        assert_eq!(id, "a430d84680aabd0b.png");
        assert!(valid_media_id(&id));
        assert!(!valid_media_id("../../x.png") && !valid_media_id("a430d84680aabd0b.") && !valid_media_id("a430d84680aabd0b"));
        assert_eq!(media_id(b"", "../e x e"), "cbf29ce484222325.exe");
        let m = encode_media(&id, b"hello");
        assert_eq!(decode_media(&m), Some((id.clone(), &b"hello"[..])));
        assert_eq!(decode_media(&encode_media("../x.png", b"x")), None);
        let p = Painting { uid: 9, pos: [-3, 70, 12], face: 5, up: 1, w: 2, h: 16, gone: false, media: id };
        assert_eq!(Painting::decode(&p.encode()), Some(p.clone()));
        assert_eq!(Painting::decode(&Painting { w: 17, ..p.clone() }.encode()), None);
        assert_eq!(Painting::decode(&Painting { media: "x".into(), ..p.clone() }.encode()), None);
        let gone = Painting { uid: 9, gone: true, ..Default::default() };
        assert_eq!(Painting::decode(&gone.encode()), Some(gone));
    }

    #[test]
    fn a_message_splits_into_whole_frames() {
        let mut buf = frame(S_TEXT, b"{}");
        buf.extend(frame(S_LEAVE, &7u32.to_le_bytes()));
        assert_eq!(split_frames(&buf), Some(vec![(S_TEXT, &b"{}"[..]), (S_LEAVE, &7u32.to_le_bytes()[..])]));
        assert_eq!(split_frames(&[]), Some(vec![]));
        assert_eq!(split_frames(&buf[..buf.len() - 1]), None);
        assert_eq!(split_frames(&[0, 0, 0, 0]), None);
    }

    #[test]
    fn video_header_round_trips() {
        let h = VideoHeader { flags: VIDEO_KEY, seq: 123456, pipeline_us: 9876 };
        assert_eq!(VideoHeader::decode(&h.encode()), Some(h));
        assert_eq!(VideoHeader::decode(&[1, 2]), None);
    }

    #[test]
    fn state_and_blocks_round_trip() {
        let s = PlayerState { pos: [1.5, 64.0, -3.25], yaw: 90.0, pitch: -10.0, flags: ON_GROUND | SNEAKING, swings: 3, held: 16 };
        assert_eq!(PlayerState::decode(&s.encode()), Some(s));
        let b = vec![((1, 2, 3), 16), ((-100, 255, 7), 0)];
        assert_eq!(decode_blocks(&encode_blocks(&b)), b);
    }

    #[test]
    fn time_and_teleport_round_trip() {
        let t = TimeSync { world_time: -5 + (1 << 40), cycle: true, snap: true };
        assert_eq!(TimeSync::decode(&t.encode()), Some(t));
        assert_eq!(TimeSync::decode(&[0; 8]), None);
        let tp = Teleport { pos: [100.5, 64.0, -3.5], yaw: 90.0, pitch: -10.0, flags: TP_REL_YAW | TP_REL_PITCH | TP_SURFACE };
        assert_eq!(Teleport::decode(&tp.encode()), Some(tp));
        let mut bad = tp.encode();
        bad[..8].copy_from_slice(&f64::NAN.to_le_bytes());
        assert_eq!(Teleport::decode(&bad), None);
    }

    #[test]
    fn inputs_round_trip_and_stop_at_garbage() {
        let e = vec![
            Input::KeyDown("KeyW".into()),
            Input::MouseMove(3.0, -2.0),
            Input::MouseDown(0),
            Input::Wheel(-1.0),
            Input::Cursor(0.5, 0.25),
            Input::MouseUp(2),
            Input::KeyUp("Space".into()),
            Input::ReleaseAll,
            Input::Seq(77),
        ];
        let mut bytes = encode_inputs(&e);
        assert_eq!(decode_inputs(&bytes), e);
        bytes.extend_from_slice(&[I_MOVE, 1, 2]);
        assert_eq!(decode_inputs(&bytes), e);
    }

    #[test]
    fn annex_b_splits_on_three_and_four_byte_codes() {
        let s = [0, 0, 0, 1, 9, 0xF0, 0, 0, 1, 0x67, 1, 2, 0, 0, 0, 1, 0x65, 5];
        let n = nal_units(&s);
        assert_eq!(n, vec![&[9, 0xF0][..], &[0x67, 1, 2][..], &[0x65, 5][..]]);
        assert_eq!(n.iter().map(|u| nal_type(u)).collect::<Vec<_>>(), vec![9, 7, 5]);
    }
}
