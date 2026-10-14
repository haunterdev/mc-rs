//! Where mc-rs keeps its files on a player's PC, shared by the launcher and the viewer.
//!
//! - `%APPDATA%\mc-rs`: `options.txt`, the downloaded client jar, `logs\`.
//! - `%LOCALAPPDATA%\Programs\mc-rs`: the installed viewer, the per-user program folder
//!   Windows apps install to.

use std::path::{Path, PathBuf};

/// `downloads.client` in Mojang's 1.12.2 version json, read 2026-10-08.
pub const CLIENT_URL: &str = "https://piston-data.mojang.com/v1/objects/0f275bc1547d01fa5f56ba34bdc87d981ee12daf/client.jar";
pub const CLIENT_SHA1: &str = "0f275bc1547d01fa5f56ba34bdc87d981ee12daf";

/// `%APPDATA%\mc-rs`.
pub fn data_dir() -> Option<PathBuf> {
    std::env::var_os("APPDATA").map(|a| PathBuf::from(a).join("mc-rs"))
}

/// `%LOCALAPPDATA%\Programs\mc-rs`.
pub fn install_dir() -> Option<PathBuf> {
    std::env::var_os("LOCALAPPDATA").map(|a| PathBuf::from(a).join("Programs").join("mc-rs"))
}

/// `%APPDATA%\mc-rs\logs`.
pub fn log_dir() -> Option<PathBuf> {
    data_dir().map(|d| d.join("logs"))
}

/// Where the launcher keeps a client jar it downloaded.
pub fn cached_jar() -> Option<PathBuf> {
    data_dir().map(|d| d.join("client-1.12.2.jar"))
}

/// Where the client jar is looked for, in order: `MCRS_CLIENT_JAR`, the Prism Launcher library
/// cache, the vanilla launcher's versions folder, then the copy the launcher downloads.
pub fn client_jar_candidates() -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Ok(p) = std::env::var("MCRS_CLIENT_JAR") {
        out.push(PathBuf::from(p));
    }
    if let Ok(appdata) = std::env::var("APPDATA") {
        let a = Path::new(&appdata);
        out.push(a.join(r"PrismLauncher\libraries\com\mojang\minecraft\1.12.2\minecraft-1.12.2-client.jar"));
        out.push(a.join(r".minecraft\versions\1.12.2\1.12.2.jar"));
    }
    if let Ok(home) = std::env::var("HOME") {
        out.push(Path::new(&home).join(".minecraft/versions/1.12.2/1.12.2.jar"));
    }
    out.extend(cached_jar());
    out
}

/// The first candidate that exists.
pub fn find_client_jar() -> Option<PathBuf> {
    client_jar_candidates().into_iter().find(|p| p.is_file())
}
