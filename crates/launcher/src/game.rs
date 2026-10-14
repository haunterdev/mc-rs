//! Starting the viewer from the window: a seed or a server to join (join.rs), the client jar,
//! and its output kept in `%APPDATA%\mc-rs\logs\latest.log` (the run before is `previous.log`).

use std::io;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};

/// The log a run writes, or `None` when `APPDATA` is unset.
pub fn log_path() -> Option<PathBuf> {
    paths::log_dir().map(|d| d.join("latest.log"))
}

/// A random seed the way the game's world screen picks one, as the text the viewer parses.
pub fn random_seed() -> String {
    use std::hash::{BuildHasher, Hasher};
    let mut h = std::collections::hash_map::RandomState::new().build_hasher();
    h.write_u128(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos()));
    (h.finish() as i64).to_string()
}

/// Starts the viewer with `args` (`--seed S`, or `--join URL`) and `env` (a join's sign-in
/// token). `MCRS_LAUNCHED` tells it to report a startup error on stderr only, where the launcher
/// reads it, instead of a message box of its own.
pub fn start(viewer: &Path, args: &[&str], env: &[(&str, &str)], jar: &Path) -> io::Result<Child> {
    let mut cmd = Command::new(viewer);
    cmd.args(args).envs(env.iter().copied()).env("MCRS_LAUNCHED", "1").env("MCRS_CLIENT_JAR", jar).env("NO_COLOR", "1");
    match log_path() {
        Some(log) => {
            if let Some(dir) = log.parent() {
                std::fs::create_dir_all(dir)?;
            }
            let _ = std::fs::rename(&log, log.with_file_name("previous.log"));
            let file = std::fs::File::create(&log)?;
            cmd.stdout(file.try_clone()?).stderr(file);
        }
        None => {
            cmd.stdout(Stdio::null()).stderr(Stdio::null());
        }
    }
    cmd.spawn()
}

/// The last `n` lines of the log with terminal color codes taken out.
pub fn log_tail(n: usize) -> Vec<String> {
    let Some(text) = log_path().and_then(|p| std::fs::read(p).ok()) else { return Vec::new() };
    let text = strip_ansi(&String::from_utf8_lossy(&text));
    let lines: Vec<String> = text.lines().filter(|l| !l.trim().is_empty()).map(str::to_owned).collect();
    lines[lines.len().saturating_sub(n)..].to_vec()
}

fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' && chars.peek() == Some(&'[') {
            chars.next();
            for c in chars.by_ref() {
                if c.is_ascii_alphabetic() {
                    break;
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// Opens a folder in Explorer, or shows a file selected in its folder. Explorer wants the
/// quotes around the path only, not around `/select,`, so the argument goes in raw.
pub fn reveal(path: &Path) {
    let mut cmd = Command::new("explorer");
    let arg = if path.is_file() {
        format!("/select,\"{}\"", path.display())
    } else {
        let _ = std::fs::create_dir_all(path);
        format!("\"{}\"", path.display())
    };
    #[cfg(windows)]
    std::os::windows::process::CommandExt::raw_arg(&mut cmd, arg);
    #[cfg(not(windows))]
    cmd.arg(arg.trim_start_matches("/select,").trim_matches('"'));
    let _ = cmd.spawn();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn color_codes_come_out() {
        assert_eq!(strip_ansi("\u{1b}[2m2026\u{1b}[0m \u{1b}[32m INFO\u{1b}[0m ok"), "2026  INFO ok");
    }

    #[test]
    fn random_seeds_parse_as_longs_and_differ() {
        let (a, b) = (random_seed(), random_seed());
        assert!(a.parse::<i64>().is_ok());
        assert_ne!(a, b);
    }
}
