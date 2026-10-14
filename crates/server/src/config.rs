//! `server.json` in the data folder, written with the defaults on the first start.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default)]
pub struct Config {
    /// The address the web page and its socket listen on. 127.0.0.1 serves this PC and the
    /// tunnel; 0.0.0.0 also serves the local network.
    pub bind: String,
    pub port: u16,
    /// The world seed, picked at random on the first start.
    pub seed: i64,
    /// New names may sign up from the login page.
    pub open_registration: bool,
    /// When set, signing up also asks for this password.
    pub join_password: String,
    /// Players may join with no account, as a guest (`open_registration` false turns guests off
    /// too; `join_password` asks them as well).
    pub guests: bool,
    pub max_players: usize,
    /// The viewer exe; empty finds it next to the server or in the launcher's install folder.
    pub viewer: String,
    /// The largest picture and frame rate a player can pick; each session renders on this PC's GPU.
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    /// The bitrate "Auto" gives 1280x720 at 60 fps, scaled by pixels and frame rate for others.
    pub bitrate_kbps: u32,
    /// The most a player can pick.
    pub max_bitrate_kbps: u32,
    /// The ffmpeg H.264 encoder: libx264 (the CPU, the default and the quickest out: 2 to 3 ms a
    /// frame), or h264_amf (AMD), h264_nvenc (NVIDIA), h264_qsv (Intel), which ffmpeg holds a
    /// frame behind.
    pub encoder: String,
    pub render_distance: i32,
    /// Chunk worker threads per session.
    pub workers: usize,
    /// Start a Cloudflare tunnel and print its public link: a quick tunnel (a new
    /// `*.trycloudflare.com` link each start), or the named tunnel below.
    pub tunnel: bool,
    /// The cloudflared exe; empty looks on PATH and in Program Files.
    pub cloudflared: String,
    /// A named tunnel (`cloudflared tunnel create NAME`, credentials in `~/.cloudflared`) to run
    /// instead of a quick tunnel; empty runs a quick tunnel.
    pub tunnel_name: String,
    /// The hostname the named tunnel serves (`cloudflared tunnel route dns NAME HOST`), the
    /// public link `https://HOST`.
    pub hostname: String,
    /// Who may run the commands that change the world or other players (`/time`, `/tp`,
    /// `/gamemode` and the rest; commands.rs): "everyone" (the default) or "ops".
    pub commands: String,
    /// Operators: the names that may run every command whatever `commands` says, and `/op` and
    /// `/deop`. The server console is always one; the first account to play on a server with
    /// none (the host's) becomes one; `/op NAME` typed in the server window, or `mc-rs-server
    /// --op NAME`, adds a name.
    pub ops: Vec<String>,
    /// Survival: "peaceful", "easy", "normal" or "hard".
    pub difficulty: String,
    /// Players may hurt each other.
    pub pvp: bool,
    /// Chunks around each player the game keeps loaded and ticking.
    pub sim_distance: i32,
    /// The most server entities (items and mobs) at once, on top of the mob caps.
    pub max_entities: usize,
    /// The 1.12.2 client jar the game reads recipes and loot tables from; empty finds one.
    pub jar: String,
    /// The game mode of a player with no record yet: "survival", "creative", "adventure" or
    /// "spectator".
    pub default_game_mode: String,
}

impl Default for Config {
    fn default() -> Config {
        Config {
            bind: "127.0.0.1".into(),
            port: 25580,
            seed: 0,
            open_registration: true,
            join_password: String::new(),
            guests: true,
            max_players: 4,
            viewer: String::new(),
            width: 1920,
            height: 1080,
            fps: 120,
            bitrate_kbps: 6000,
            max_bitrate_kbps: 30000,
            encoder: "libx264".into(),
            render_distance: 8,
            workers: 3,
            tunnel: true,
            cloudflared: String::new(),
            tunnel_name: String::new(),
            hostname: String::new(),
            commands: "everyone".into(),
            ops: Vec::new(),
            difficulty: "normal".into(),
            pvp: true,
            sim_distance: 8,
            max_entities: 300,
            jar: String::new(),
            default_game_mode: "survival".into(),
        }
    }
}

pub fn random_u64() -> u64 {
    let mut b = [0u8; 8];
    getrandom::fill(&mut b).expect("system random source");
    u64::from_le_bytes(b)
}

impl Config {
    /// Reads `server.json`, creating it (with a fresh seed) when missing.
    pub fn load(dir: &Path) -> std::io::Result<Config> {
        let path = dir.join("server.json");
        match std::fs::read_to_string(&path) {
            Ok(text) => serde_json::from_str(&text).map_err(|e| std::io::Error::other(format!("{}: {e}", path.display()))),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                let c = Config { seed: random_u64() as i64, ..Config::default() };
                std::fs::create_dir_all(dir)?;
                std::fs::write(&path, serde_json::to_string_pretty(&c).unwrap())?;
                Ok(c)
            }
            Err(e) => Err(e),
        }
    }

    /// Writes the operator list into `server.json`, keeping every other setting as the file has it.
    pub fn save_ops(dir: &Path, ops: &[String]) -> std::io::Result<()> {
        let path = dir.join("server.json");
        let text = std::fs::read_to_string(&path)?;
        let mut v: serde_json::Value = serde_json::from_str(&text).map_err(|e| std::io::Error::other(format!("{}: {e}", path.display())))?;
        let Some(o) = v.as_object_mut() else { return Err(std::io::Error::other(format!("{}: not an object", path.display()))) };
        o.insert("ops".into(), serde_json::json!(ops));
        std::fs::write(&path, serde_json::to_string_pretty(&v).unwrap())
    }

    /// Whether every signed-in player may run every command but `/op` and `/deop`.
    pub fn commands_open(&self) -> bool {
        !self.commands.eq_ignore_ascii_case("ops")
    }

    /// The viewer exe to start sessions with.
    pub fn viewer_path(&self) -> Option<PathBuf> {
        if !self.viewer.is_empty() {
            return Some(PathBuf::from(&self.viewer));
        }
        let exe = |n: &str| format!("{n}{}", std::env::consts::EXE_SUFFIX);
        let mut candidates = Vec::new();
        if let Ok(me) = std::env::current_exe() {
            if let Some(dir) = me.parent() {
                candidates.push(dir.join(exe("viewer")));
                candidates.push(dir.join(exe("mc-rs-viewer")));
            }
        }
        if let Some(dir) = paths::install_dir() {
            candidates.push(dir.join(exe("mc-rs-viewer")));
        }
        candidates.into_iter().find(|p| p.is_file())
    }

    /// The picture a player asked for, within the configured maximum: 16:9, width a multiple of
    /// 64; 1280x720 at 60 fps when they asked nothing; bitrate "Auto" (0) scales with pixels and
    /// frame rate.
    pub fn picture(&self, width: Option<u32>, fps: Option<u32>, bitrate_kbps: Option<u32>) -> Picture {
        let max_w = self.width.max(320);
        let w = width.unwrap_or(1280.min(max_w)).clamp(320, max_w) / 64 * 64;
        let h = (w as u64 * self.height as u64 / max_w as u64) as u32 / 2 * 2;
        let fps = fps.unwrap_or(60).clamp(15, self.fps.max(15));
        let auto = (self.bitrate_kbps as u64 * w as u64 * h as u64 * fps as u64 / (1280 * 720 * 60)) as u32;
        let bitrate_kbps = match bitrate_kbps {
            Some(b) if b > 0 => b,
            _ => auto,
        }
        .clamp(500, self.max_bitrate_kbps.max(500));
        Picture { width: w, height: h.max(180), fps, bitrate_kbps }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Picture {
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub bitrate_kbps: u32,
}

/// `%APPDATA%\mc-rs\server`, or `~/.mc-rs-server` where there is no APPDATA.
pub fn default_dir() -> PathBuf {
    paths::data_dir()
        .map(|d| d.join("server"))
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".mc-rs-server")))
        .unwrap_or_else(|| PathBuf::from("mc-rs-server"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picture_stays_within_the_maximum() {
        let c = Config::default();
        let p = |w, f, b| {
            let p = c.picture(w, f, b);
            (p.width, p.height, p.fps, p.bitrate_kbps)
        };
        assert_eq!(p(None, None, None), (1280, 720, 60, 6000));
        assert_eq!(p(Some(640), Some(30), None), (640, 360, 30, 750));
        assert_eq!(p(Some(4000), Some(240), Some(99999)), (1920, 1080, 120, 30000));
        assert_eq!(p(Some(1000), None, Some(3000)).0, 960);
        assert_eq!(p(Some(1000), None, Some(3000)).3, 3000);
    }

    #[test]
    fn missing_fields_take_defaults() {
        let c: Config = serde_json::from_str(r#"{"port": 1234}"#).unwrap();
        assert_eq!(c.port, 1234);
        assert_eq!(c.encoder, "libx264");
        assert!(c.commands_open() && c.ops.is_empty() && c.guests);
        assert_eq!((c.difficulty.as_str(), c.pvp, c.sim_distance, c.max_entities), ("normal", true, 8, 300));
        assert_eq!((c.jar.as_str(), c.default_game_mode.as_str()), ("", "survival"));
        let c: Config = serde_json::from_str(r#"{"difficulty": "hard", "pvp": false, "sim_distance": 4}"#).unwrap();
        assert_eq!((c.difficulty.as_str(), c.pvp, c.sim_distance, c.max_entities), ("hard", false, 4, 300));
        let c: Config = serde_json::from_str(r#"{"commands": "ops", "ops": ["xy"]}"#).unwrap();
        assert!(!c.commands_open());
    }
}
