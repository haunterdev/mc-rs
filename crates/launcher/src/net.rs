//! HTTPS in-process through ureq and rustls: the client jar from Mojang, the update channel and a
//! server's sign-in (join.rs).
//! Every download goes to a `.part` file and is kept only when its hash matches.

use std::io::{self, Read, Write};
use std::path::Path;
use std::time::Duration;

#[derive(Clone, Copy)]
pub enum Hash<'a> {
    Sha1(&'a str),
    Sha256(&'a str),
}

pub fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_connect(Some(Duration::from_secs(15)))
        .timeout_recv_response(Some(Duration::from_secs(30)))
        .user_agent(concat!("mc-rs-launcher/", env!("CARGO_PKG_VERSION")))
        .build()
        .into()
}

fn net_err(e: ureq::Error) -> io::Error {
    match e {
        ureq::Error::Io(e) => e,
        ureq::Error::StatusCode(404) => io::Error::new(io::ErrorKind::NotFound, "the server answered HTTP 404"),
        ureq::Error::StatusCode(code) => io::Error::other(format!("the server answered HTTP {code}")),
        e => io::Error::other(e.to_string()),
    }
}

/// A small JSON document, such as the update manifest.
pub fn get_json(url: &str) -> io::Result<serde_json::Value> {
    let mut resp = agent().get(url).call().map_err(net_err)?;
    let text = resp.body_mut().with_config().limit(1 << 20).read_to_string().map_err(net_err)?;
    serde_json::from_str(&text).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}

/// Downloads `url` to `dest`. `progress` gets the bytes so far and the total when the server
/// sent one.
pub fn download(url: &str, dest: &Path, hash: Hash, progress: &dyn Fn(u64, Option<u64>)) -> io::Result<()> {
    if let Some(dir) = dest.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let mut resp = agent().get(url).call().map_err(net_err)?;
    let total = resp.body().content_length();
    let part = dest.with_extension("part");
    let result = (|| {
        let mut reader = resp.body_mut().with_config().limit(u64::MAX).reader();
        let mut file = io::BufWriter::new(std::fs::File::create(&part)?);
        let (mut ctx, expected) = match hash {
            Hash::Sha1(h) => (ring::digest::Context::new(&ring::digest::SHA1_FOR_LEGACY_USE_ONLY), h),
            Hash::Sha256(h) => (ring::digest::Context::new(&ring::digest::SHA256), h),
        };
        let mut buf = vec![0u8; 1 << 16];
        let mut done = 0u64;
        loop {
            let n = reader.read(&mut buf)?;
            if n == 0 {
                break;
            }
            ctx.update(&buf[..n]);
            file.write_all(&buf[..n])?;
            done += n as u64;
            progress(done, total);
        }
        file.into_inner().map_err(|e| e.into_error())?.sync_all()?;
        let got = hex(ctx.finish().as_ref());
        if !got.eq_ignore_ascii_case(expected) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("the download did not match its checksum ({got}, expected {expected})"),
            ));
        }
        std::fs::rename(&part, dest)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&part);
    }
    result
}

pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// SHA-256 of a file, for tests and tools/dist.py's manifest check.
#[cfg(test)]
pub fn sha256_file(path: &Path) -> io::Result<String> {
    Ok(hex(ring::digest::digest(&ring::digest::SHA256, &std::fs::read(path)?).as_ref()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_client_matches_the_pinned_hash() {
        let Some(p) = paths::find_client_jar() else {
            eprintln!("skipped: no client jar");
            return;
        };
        let sha1 = hex(ring::digest::digest(&ring::digest::SHA1_FOR_LEGACY_USE_ONLY, &std::fs::read(&p).unwrap()).as_ref());
        assert_eq!(sha1, paths::CLIENT_SHA1, "{}", p.display());
    }

    #[test]
    fn hex_is_lowercase_pairs() {
        assert_eq!(hex(&[0, 0xab, 0x10]), "00ab10");
    }

    /// Fetches the real client jar from Mojang into the temp folder, so it needs the network:
    /// `cargo test --release -p launcher -- --ignored`.
    #[test]
    #[ignore]
    fn downloads_and_verifies_the_client_jar() {
        let dir = std::env::temp_dir().join(format!("mc-rs-fetch-test-{}", std::process::id()));
        let dest = dir.join("client-1.12.2.jar");
        let last = std::cell::Cell::new((0, None));
        download(paths::CLIENT_URL, &dest, Hash::Sha1(paths::CLIENT_SHA1), &|d, t| last.set((d, t))).unwrap();
        let (done, total) = last.get();
        assert_eq!(Some(done), total);
        assert_eq!(std::fs::metadata(&dest).unwrap().len(), done);
        assert!(!dest.with_extension("part").exists());

        // A wrong hash leaves nothing behind.
        let bad = dir.join("bad.jar");
        let err = download(paths::CLIENT_URL, &bad, Hash::Sha256("00"), &|_, _| {}).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
        assert!(!bad.exists() && !bad.with_extension("part").exists());
        assert_eq!(sha256_file(&dest).unwrap().len(), 64);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
