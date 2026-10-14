//! The update channel. tools/dist.py bakes a manifest URL into the launcher (`MCRS_UPDATE_URL`)
//! and writes the matching `dist/manifest.json`:
//!
//! ```json
//! { "version": "0.3.0", "url": "https://.../mc-rs-launcher.exe", "sha256": "...", "size": 9000000,
//!   "notes": "what changed" }
//! ```
//!
//! An update is a whole new `mc-rs-launcher.exe`, which carries its own viewer. It downloads beside the
//! running exe, the running exe is renamed to `<name>.old` (Windows allows renaming a running
//! exe, not deleting it), the new one takes its name and starts, and the next start deletes the
//! `.old`.

use std::io;
use std::path::{Path, PathBuf};

use crate::net;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
pub const URL: Option<&str> = option_env!("MCRS_UPDATE_URL");

#[derive(Clone, Debug, PartialEq)]
pub struct Manifest {
    pub version: String,
    pub url: String,
    pub sha256: String,
    pub size: Option<u64>,
    pub notes: String,
}

impl Manifest {
    pub fn parse(v: &serde_json::Value) -> io::Result<Manifest> {
        let field = |k: &str| {
            v.get(k).and_then(|s| s.as_str()).map(str::to_owned).ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidData, format!("the update manifest has no \"{k}\""))
            })
        };
        let m = Manifest {
            version: field("version")?,
            url: field("url")?,
            sha256: field("sha256")?,
            size: v.get("size").and_then(|s| s.as_u64()),
            notes: v.get("notes").and_then(|s| s.as_str()).unwrap_or_default().to_owned(),
        };
        if !m.url.starts_with("https://") {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "the update URL is not https"));
        }
        Ok(m)
    }
}

/// `a.b.c` compared part by part as numbers; anything after a `-` or `+` is ignored.
pub fn newer(candidate: &str, current: &str) -> bool {
    let parts = |s: &str| -> Vec<u64> {
        s.trim_start_matches('v').split(['-', '+']).next().unwrap_or("").split('.').map(|n| n.parse().unwrap_or(0)).collect()
    };
    let (mut a, mut b) = (parts(candidate), parts(current));
    let len = a.len().max(b.len());
    a.resize(len, 0);
    b.resize(len, 0);
    a > b
}

/// The manifest when it names a newer version, `None` when this is current, no channel is set,
/// or the channel has no release yet (HTTP 404).
pub fn check() -> io::Result<Option<Manifest>> {
    let Some(url) = URL else { return Ok(None) };
    let json = match net::get_json(url) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        r => r?,
    };
    let m = Manifest::parse(&json)?;
    Ok(newer(&m.version, VERSION).then_some(m))
}

/// Downloads and verifies the new exe, swaps it in for the running one and starts it. The caller
/// exits once this returns `Ok`.
pub fn apply(m: &Manifest, progress: &dyn Fn(u64, Option<u64>)) -> io::Result<()> {
    let exe = std::env::current_exe()?;
    let new = sibling(&exe, "new");
    net::download(&m.url, &new, net::Hash::Sha256(&m.sha256), &|d, t| progress(d, t.or(m.size)))?;
    swap(&exe, &new)?;
    std::process::Command::new(&exe).arg("--updated-from").arg(VERSION).spawn()?;
    Ok(())
}

/// `mc-rs-launcher.exe` -> `mc-rs-launcher.exe.<tag>`.
fn sibling(exe: &Path, tag: &str) -> PathBuf {
    let mut name = exe.file_name().unwrap_or_default().to_os_string();
    name.push(".");
    name.push(tag);
    exe.with_file_name(name)
}

fn swap(exe: &Path, new: &Path) -> io::Result<()> {
    let old = sibling(exe, "old");
    let _ = std::fs::remove_file(&old);
    std::fs::rename(exe, &old)?;
    if let Err(e) = std::fs::rename(new, exe) {
        let _ = std::fs::rename(&old, exe);
        return Err(e);
    }
    Ok(())
}

/// Deletes the `.old` a self-update left. The process that renamed it may still be exiting, so
/// this retries for a few seconds on its own thread.
pub fn clean_up_after_update() {
    let Ok(exe) = std::env::current_exe() else { return };
    let old = sibling(&exe, "old");
    if !old.exists() {
        return;
    }
    std::thread::spawn(move || {
        for _ in 0..20 {
            if std::fs::remove_file(&old).is_ok() || !old.exists() {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(250));
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_compare_as_numbers() {
        assert!(newer("0.10.0", "0.9.9"));
        assert!(newer("v0.2.1", "0.2.0"));
        assert!(newer("1", "0.9"));
        assert!(!newer("0.2.0", "0.2.0"));
        assert!(!newer("0.2", "0.2.0"));
        assert!(!newer("0.2.0-beta", "0.2.0"));
        assert!(!newer("0.1.9", "0.2.0"));
    }

    #[test]
    fn manifest_needs_https_and_its_fields() {
        let ok = serde_json::json!({"version": "0.3.0", "url": "https://x/mc-rs-launcher.exe", "sha256": "ab", "size": 5});
        let m = Manifest::parse(&ok).unwrap();
        assert_eq!((m.version.as_str(), m.size, m.notes.as_str()), ("0.3.0", Some(5), ""));
        let plain = serde_json::json!({"version": "0.3.0", "url": "http://x/mc-rs-launcher.exe", "sha256": "ab"});
        assert!(Manifest::parse(&plain).is_err());
        let missing = serde_json::json!({"version": "0.3.0", "url": "https://x/mc-rs-launcher.exe"});
        assert!(Manifest::parse(&missing).unwrap_err().to_string().contains("sha256"));
    }

    #[test]
    fn swap_puts_the_new_exe_in_place_and_keeps_the_old() {
        let dir = std::env::temp_dir().join(format!("mc-rs-swap-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let exe = dir.join("mc-rs-launcher.exe");
        let new = sibling(&exe, "new");
        std::fs::write(&exe, "old build").unwrap();
        std::fs::write(&new, "new build").unwrap();
        swap(&exe, &new).unwrap();
        assert_eq!(std::fs::read_to_string(&exe).unwrap(), "new build");
        assert_eq!(std::fs::read_to_string(dir.join("mc-rs-launcher.exe.old")).unwrap(), "old build");
        assert!(!new.exists());

        // A failed second rename puts the running exe back.
        let err = swap(&exe, &dir.join("missing")).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::NotFound);
        assert_eq!(std::fs::read_to_string(&exe).unwrap(), "new build");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
