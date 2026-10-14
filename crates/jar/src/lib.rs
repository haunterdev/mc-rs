//! Reads assets out of the local Minecraft 1.12.2 client jar at runtime. Nothing read here is
//! written to disk or committed; the viewer keeps the decoded data in memory only.
//!
//! The jar is a plain zip. This reads the central directory and inflates entries with
//! `miniz_oxide`; only the stored and deflate methods occur in the 1.12.2 jar.

use std::collections::HashMap;
use std::io;
use std::path::Path;

struct Entry {
    method: u16,
    compressed: usize,
    size: usize,
    local_header: usize,
}

pub struct Jar {
    data: Vec<u8>,
    entries: HashMap<String, Entry>,
}

fn u16_at(b: &[u8], i: usize) -> u16 {
    u16::from_le_bytes([b[i], b[i + 1]])
}

fn u32_at(b: &[u8], i: usize) -> u32 {
    u32::from_le_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]])
}

fn bad(msg: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, msg.to_string())
}

impl Jar {
    /// Opens the first candidate that exists.
    pub fn find() -> io::Result<Jar> {
        for p in paths::client_jar_candidates() {
            if p.is_file() {
                return Jar::open(&p);
            }
        }
        Err(io::Error::new(
            io::ErrorKind::NotFound,
            "no Minecraft 1.12.2 client jar found; start mc-rs-launcher.exe, which downloads it, or set MCRS_CLIENT_JAR to its path",
        ))
    }

    pub fn open(path: &Path) -> io::Result<Jar> {
        let data = std::fs::read(path)?;
        // End of central directory record: the last "PK\x05\x06" within 64 KiB of the end.
        let lo = data.len().saturating_sub(22 + 65535);
        let eocd = (lo..=data.len().saturating_sub(22))
            .rev()
            .find(|&i| u32_at(&data, i) == 0x0605_4b50)
            .ok_or_else(|| bad("not a zip file"))?;
        let count = u16_at(&data, eocd + 10) as usize;
        let mut at = u32_at(&data, eocd + 16) as usize;
        let mut entries = HashMap::with_capacity(count);
        for _ in 0..count {
            if at + 46 > data.len() || u32_at(&data, at) != 0x0201_4b50 {
                return Err(bad("broken central directory"));
            }
            let name_len = u16_at(&data, at + 28) as usize;
            let extra_len = u16_at(&data, at + 30) as usize;
            let comment_len = u16_at(&data, at + 32) as usize;
            let name = String::from_utf8_lossy(&data[at + 46..at + 46 + name_len]).into_owned();
            entries.insert(
                name,
                Entry {
                    method: u16_at(&data, at + 10),
                    compressed: u32_at(&data, at + 20) as usize,
                    size: u32_at(&data, at + 24) as usize,
                    local_header: u32_at(&data, at + 42) as usize,
                },
            );
            at += 46 + name_len + extra_len + comment_len;
        }
        Ok(Jar { data, entries })
    }

    pub fn contains(&self, name: &str) -> bool {
        self.entries.contains_key(name)
    }

    /// The bytes of one entry, inflated.
    pub fn read(&self, name: &str) -> io::Result<Vec<u8>> {
        let e = self.entries.get(name).ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, name.to_string()))?;
        let h = e.local_header;
        if u32_at(&self.data, h) != 0x0403_4b50 {
            return Err(bad("broken local header"));
        }
        let start = h + 30 + u16_at(&self.data, h + 26) as usize + u16_at(&self.data, h + 28) as usize;
        let raw = self.data.get(start..start + e.compressed).ok_or_else(|| bad("entry past end of file"))?;
        let out = match e.method {
            0 => raw.to_vec(),
            8 => miniz_oxide::inflate::decompress_to_vec(raw).map_err(|_| bad("bad deflate stream"))?,
            m => return Err(bad(&format!("zip method {m}"))),
        };
        if out.len() != e.size {
            return Err(bad("size mismatch"));
        }
        Ok(out)
    }

    /// `assets/minecraft/<rel>`.
    pub fn asset(&self, rel: &str) -> io::Result<Vec<u8>> {
        self.read(&format!("assets/minecraft/{rel}"))
    }

    pub fn json(&self, rel: &str) -> io::Result<serde_json::Value> {
        serde_json::from_slice(&self.asset(rel)?).map_err(|e| bad(&format!("{rel}: {e}")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_client_jar() {
        let Ok(jar) = Jar::find() else {
            eprintln!("skipped: no client jar");
            return;
        };
        let stone = jar.asset("textures/blocks/stone.png").expect("stone.png");
        assert_eq!(&stone[..8], b"\x89PNG\r\n\x1a\n");
        let model = jar.json("models/block/cube_all.json").expect("cube_all.json");
        assert_eq!(model["parent"], "block/cube");
    }
}
