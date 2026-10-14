//! Decoding for media.rs: PNG and GIF with the viewer's own decoders, JPEG, WebP and BMP through
//! Bevy's image decoders, and video (MP4, WebM and the rest ffmpeg reads) through an `ffmpeg`
//! child that writes raw frames at the video's own pace, looping, the way encode.rs runs one for
//! the stream. The newest frame waits in [`Video::latest`] for the painting to show it.

use std::io::{self, Read};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};

use bevy::asset::RenderAssetUsages;
use bevy::image::{CompressedImageFormats, Image, ImageSampler, ImageType};

use crate::media::{decode, MAX_SIDE};
use crate::textures::Argb;

/// Files ffmpeg plays as video.
pub const VIDEO: [&str; 11] = ["mp4", "webm", "mkv", "mov", "avi", "m4v", "ogv", "wmv", "flv", "mpg", "mpeg"];
/// Stills Bevy's decoders read.
pub const STILL: [&str; 4] = ["jpg", "jpeg", "webp", "bmp"];
/// The longer side of a video painting's frames, in pixels; a painting a few blocks wide shows
/// no more, and every streamed session decodes its own copy.
pub const VIDEO_SIDE: u32 = 480;
/// Frames a second a video painting shows at most.
pub const VIDEO_FPS: u32 = 30;

pub enum Content {
    /// A still is one frame; a GIF's frames carry their delays in seconds.
    Frames(Vec<(Argb, f32)>),
    Video(Video),
}

impl Content {
    pub fn size(&self) -> (u32, u32) {
        match self {
            Content::Frames(f) => (f[0].0.width, f[0].0.height),
            Content::Video(v) => (v.width, v.height),
        }
    }
}

/// A playing video: ffmpeg's frames, RGBA, `width` by `height`.
pub struct Video {
    pub width: u32,
    pub height: u32,
    pub latest: Arc<Mutex<Option<Vec<u8>>>>,
    child: Child,
}

impl Drop for Video {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

pub fn ext_of(path: &Path) -> String {
    path.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase()
}

/// What a media file shows. Video starts playing here.
pub fn load(path: &Path) -> io::Result<Content> {
    let ext = ext_of(path);
    if VIDEO.contains(&ext.as_str()) {
        return start_video(path).map(Content::Video);
    }
    if STILL.contains(&ext.as_str()) {
        return still(&std::fs::read(path)?, &ext).map(|a| Content::Frames(vec![(a, 0.0)]));
    }
    decode(path).map(Content::Frames)
}

fn bad(e: impl std::fmt::Display) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, e.to_string())
}

/// A JPEG, WebP or BMP as one frame.
pub fn still(bytes: &[u8], ext: &str) -> io::Result<Argb> {
    let image = Image::from_buffer(bytes, ImageType::Extension(ext), CompressedImageFormats::NONE, false, ImageSampler::Default, RenderAssetUsages::MAIN_WORLD)
        .map_err(bad)?;
    let (width, height) = (image.width(), image.height());
    if width == 0 || height == 0 || width > MAX_SIDE || height > MAX_SIDE {
        return Err(bad(format!("{width}x{height} is outside 1 to {MAX_SIDE} pixels a side")));
    }
    let data = image.data.ok_or_else(|| bad("no pixels"))?;
    if data.len() != (width * height * 4) as usize {
        return Err(bad(format!("{ext}: {} bytes for {width}x{height}", data.len())));
    }
    let pixels = data.chunks_exact(4).map(|p| (p[3] as u32) << 24 | (p[0] as u32) << 16 | (p[1] as u32) << 8 | p[2] as u32).collect();
    Ok(Argb { width, height, pixels })
}

fn ffmpeg() -> Command {
    let mut cmd = Command::new(std::env::var("MCRS_FFMPEG").unwrap_or_else(|_| "ffmpeg".into()));
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // CREATE_NO_WINDOW
        cmd.creation_flags(0x0800_0000);
    }
    cmd
}

/// The size of the first video stream in ffmpeg's description of the file, as
/// `Stream #0:0: Video: h264 (High), yuv420p, 1920x1080 [SAR 1:1 DAR 16:9], 30 fps`.
pub fn video_size(info: &str) -> Option<(u32, u32)> {
    let line = info.lines().find(|l| l.contains("Video:"))?;
    line.split(|c: char| c == ',' || c == ' ' || c == '[').find_map(|t| {
        let (w, h) = t.split_once('x')?;
        let (w, h) = (w.parse::<u32>().ok()?, h.parse::<u32>().ok()?);
        (w > 0 && h > 0).then_some((w, h))
    })
}

/// A `w` by `h` video scaled so its longer side is at most `side`, both sides even.
pub fn scaled(w: u32, h: u32, side: u32) -> (u32, u32) {
    let s = (side as f32 / w.max(h) as f32).min(1.0);
    let even = |v: u32| ((v as f32 * s / 2.0).round() as u32 * 2).max(2);
    (even(w), even(h))
}

fn start_video(path: &Path) -> io::Result<Video> {
    let info = ffmpeg().args(["-hide_banner", "-nostdin", "-i"]).arg(path).stdin(Stdio::null()).stdout(Stdio::null()).output();
    let info = info.map_err(|e| io::Error::new(e.kind(), format!("video needs ffmpeg: {e}")))?;
    let (w, h) = video_size(&String::from_utf8_lossy(&info.stderr)).ok_or_else(|| bad("ffmpeg found no video stream"))?;
    let (width, height) = scaled(w, h, VIDEO_SIDE);
    let mut child = ffmpeg()
        .args(["-hide_banner", "-loglevel", "error", "-nostdin", "-stream_loop", "-1", "-re", "-i"])
        .arg(path)
        .args(["-an", "-sn", "-vf", &format!("scale={width}:{height}:flags=bilinear,fps={VIDEO_FPS}"), "-pix_fmt", "rgba", "-f", "rawvideo", "pipe:1"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()?;
    let mut out = child.stdout.take().expect("piped stdout");
    let latest = Arc::new(Mutex::new(None));
    let slot = latest.clone();
    let size = (width * height * 4) as usize;
    std::thread::spawn(move || loop {
        let mut frame = vec![0u8; size];
        if out.read_exact(&mut frame).is_err() {
            break;
        }
        *slot.lock().unwrap() = Some(frame);
    });
    Ok(Video { width, height, latest, child })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn video_size_and_scale() {
        let info = "Input #0, mov,mp4,m4a,3gp,3g2,mj2, from 'a.mp4':\n  Duration: 00:00:05.00, start: 0.000000, bitrate: 1205 kb/s\n  Stream #0:0[0x1](und): Video: h264 (High) (avc1 / 0x31637661), yuv420p(progressive), 1920x1080 [SAR 1:1 DAR 16:9], 1200 kb/s, 30 fps\n  Stream #0:1[0x2](und): Audio: aac (LC), 48000 Hz, stereo";
        assert_eq!(video_size(info), Some((1920, 1080)));
        assert_eq!(video_size("Stream #0:0: Video: vp9 (Profile 0), yuv420p(tv), 640x360, SAR 1:1 DAR 16:9"), Some((640, 360)));
        assert_eq!(video_size("Stream #0:0: Audio: mp3, 44100 Hz"), None);
        assert_eq!(scaled(1920, 1080, 480), (480, 270));
        assert_eq!(scaled(1080, 1920, 480), (270, 480));
        assert_eq!(scaled(321, 101, 480), (322, 102));
    }

    #[test]
    fn stills_decode_through_bevy() {
        // A 2x1 BMP, 24-bit: red then blue (rows are BGR, bottom up, padded to 4 bytes).
        let mut bmp = Vec::new();
        bmp.extend_from_slice(b"BM");
        bmp.extend_from_slice(&(54u32 + 8).to_le_bytes());
        bmp.extend_from_slice(&[0, 0, 0, 0]);
        bmp.extend_from_slice(&54u32.to_le_bytes());
        bmp.extend_from_slice(&40u32.to_le_bytes());
        bmp.extend_from_slice(&2i32.to_le_bytes());
        bmp.extend_from_slice(&1i32.to_le_bytes());
        bmp.extend_from_slice(&1u16.to_le_bytes());
        bmp.extend_from_slice(&24u16.to_le_bytes());
        bmp.extend_from_slice(&[0; 24]);
        bmp.extend_from_slice(&[0, 0, 255, 255, 0, 0, 0, 0]);
        let a = still(&bmp, "bmp").unwrap();
        assert_eq!((a.width, a.height), (2, 1));
        assert_eq!(a.pixels, vec![0xffff_0000, 0xff00_00ff]);
        assert!(still(b"not an image", "jpg").is_err());
    }
}
