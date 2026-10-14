//! The viewer this launcher carries, zstd-compressed (tools/dist.py), and its install to
//! `%LOCALAPPDATA%\Programs\mc-rs\mc-rs-viewer.exe`. A stamp file beside it holds the payload id,
//! so a newer launcher installs its own viewer over an older one.

use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

static PAYLOAD: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/viewer.zst"));
const PAYLOAD_ID: &str = env!("MCRS_PAYLOAD_ID");

pub const VIEWER: &str = "mc-rs-viewer.exe";
const STAMP: &str = "mc-rs-viewer.id";

/// Whether this build carries a viewer (tools/dist.py builds do).
pub fn carried() -> bool {
    !PAYLOAD.is_empty()
}

/// The installed viewer when it is this launcher's build.
pub fn installed() -> Option<PathBuf> {
    installed_in(&paths::install_dir()?, PAYLOAD_ID)
}

fn installed_in(dir: &Path, id: &str) -> Option<PathBuf> {
    let exe = dir.join(VIEWER);
    let stamp = std::fs::read_to_string(dir.join(STAMP)).ok()?;
    (exe.is_file() && stamp.trim() == id).then_some(exe)
}

/// Installs the carried viewer unless it already is, and returns its path. `progress` gets the
/// compressed bytes read so far and the total.
pub fn install(progress: &dyn Fn(u64, u64)) -> io::Result<PathBuf> {
    if PAYLOAD.is_empty() {
        return Err(io::Error::other("this build carries no viewer; build it with tools/dist.py"));
    }
    if let Some(exe) = installed() {
        return Ok(exe);
    }
    let dir = paths::install_dir().ok_or_else(|| io::Error::other("LOCALAPPDATA is not set"))?;
    let exe = install_into(&dir, PAYLOAD, PAYLOAD_ID, progress)?;
    remove_legacy();
    Ok(exe)
}

fn install_into(dir: &Path, payload: &[u8], id: &str, progress: &dyn Fn(u64, u64)) -> io::Result<PathBuf> {
    std::fs::create_dir_all(dir)?;
    let exe = dir.join(VIEWER);
    let part = dir.join("mc-rs-viewer.part");
    let total = payload.len() as u64;
    let mut source = Counting { data: payload, pos: 0 };
    {
        let mut decoder = ruzstd::decoding::StreamingDecoder::new(&mut source)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))?;
        let mut file = io::BufWriter::new(std::fs::File::create(&part)?);
        let mut buf = vec![0u8; 1 << 18];
        loop {
            let n = decoder.read(&mut buf)?;
            if n == 0 {
                break;
            }
            file.write_all(&buf[..n])?;
            progress(decoder.get_ref().pos as u64, total);
        }
        file.into_inner().map_err(|e| e.into_error())?.sync_all()?;
    }
    // A running viewer cannot be deleted but can be renamed out of the way.
    if exe.exists() && std::fs::remove_file(&exe).is_err() {
        let old = dir.join("mc-rs-viewer.old");
        let _ = std::fs::remove_file(&old);
        std::fs::rename(&exe, &old)?;
    }
    std::fs::rename(&part, &exe)?;
    std::fs::write(dir.join(STAMP), id)?;
    let _ = std::fs::remove_file(dir.join("mc-rs-viewer.old"));
    progress(total, total);
    Ok(exe)
}

/// 0.1.0 unpacked to `%LOCALAPPDATA%\mc-rs\viewer-<id>.exe`; that folder goes once the new
/// install is in place.
fn remove_legacy() {
    let Some(local) = std::env::var_os("LOCALAPPDATA") else { return };
    let dir = PathBuf::from(local).join("mc-rs");
    let Ok(entries) = std::fs::read_dir(&dir) else { return };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with("viewer-") && (name.ends_with(".exe") || name.ends_with(".part")) {
            let _ = std::fs::remove_file(entry.path());
        }
    }
    let _ = std::fs::remove_dir(&dir);
}

struct Counting<'a> {
    data: &'a [u8],
    pos: usize,
}

impl Read for Counting<'_> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let n = (&self.data[self.pos..]).read(buf)?;
        self.pos += n;
        Ok(n)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    /// A zstd frame of one raw block, which ruzstd decodes like any other.
    fn frame(content: &[u8]) -> Vec<u8> {
        let mut f = vec![0x28, 0xb5, 0x2f, 0xfd];
        f.push(0x20); // single segment, 1-byte content size
        f.push(content.len() as u8);
        let header = ((content.len() as u32) << 3) | 1; // raw block, last
        f.extend(&header.to_le_bytes()[..3]);
        f.extend(content);
        f
    }

    #[test]
    fn installs_stamps_and_replaces() {
        let dir = std::env::temp_dir().join(format!("mc-rs-install-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let seen = Cell::new(0);
        let exe = install_into(&dir, &frame(b"first viewer"), "a", &|done, total| {
            assert!(done <= total);
            seen.set(done);
        })
        .unwrap();
        assert_eq!(std::fs::read(&exe).unwrap(), b"first viewer");
        assert_eq!(installed_in(&dir, "a"), Some(exe.clone()));
        assert_eq!(installed_in(&dir, "b"), None);
        assert!(seen.get() > 0);

        install_into(&dir, &frame(b"second"), "b", &|_, _| {}).unwrap();
        assert_eq!(std::fs::read(&exe).unwrap(), b"second");
        assert_eq!(installed_in(&dir, "b"), Some(exe));
        assert!(!dir.join("mc-rs-viewer.part").exists());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
