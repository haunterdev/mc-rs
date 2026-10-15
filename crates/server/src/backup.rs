//! World backups and rollback. A backup is a copy of the saved world in `backups/<name>` inside
//! the data folder, named by the local time it was made (`20261014-151113`): `edits.bin`,
//! `paintings.bin`, every `.json` file (the players, the world file, each game module's file,
//! the accounts, tokens, bans and `server.json`) and the pictures in `media/` (hard links where
//! the disk allows, since a picture file never changes once written).
//!
//! The server makes one every `backup_minutes` while anyone played since the last one, and on
//! `/backup`; before copying, the game thread writes its module files and the world writes its
//! players and clock, so the copy is the world as it is now. Past `backup_keep` the oldest go.
//!
//! A rollback happens at start, before the world loads: `--rollback NAME`, or the name `/rollback
//! NAME` left in `backups/rollback.txt`. It first backs up the world as it is (`<time>-before-rollback`),
//! then replaces the world files with the backup's. The accounts, tokens, bans and `server.json`
//! stay as they are; the backup still holds its copies of them.

use std::io;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::hub::Hub;

/// The folder in the data folder that holds the backups.
pub const DIR: &str = "backups";
/// The rollback `/rollback NAME` asks for at the next start.
const PENDING: &str = "rollback.txt";
/// Kept as they are by a rollback: who may sign in, the bans and the settings.
const KEPT: &[&str] = &["accounts.json", "tokens.json", "bans.json", "server.json"];
/// The pictures players hung, by content hash.
const MEDIA: &str = "media";
/// One file per player (game/playerdata.rs).
const PLAYERDATA: &str = "playerdata";
/// A backup being written; never listed, removed by the next prune.
const PARTIAL: &str = ".partial";

/// Whether a file in the data folder is part of the saved world.
fn saved_file(name: &str) -> bool {
    name == "edits.bin" || name == "paintings.bin" || name.ends_with(".json")
}

fn root(dir: &Path) -> PathBuf {
    dir.join(DIR)
}

/// A backup name as typed: one folder name, nothing that leaves the backups folder.
fn valid_name(name: &str) -> bool {
    !name.is_empty() && name != "." && name != ".." && !name.ends_with(PARTIAL) && !name.contains(['/', '\\', ':'])
}

/// The local time as `YYYYMMDD-HHMMSS`.
pub fn stamp() -> String {
    let [y, mo, d, h, mi, s] = local_now();
    format!("{y:04}{mo:02}{d:02}-{h:02}{mi:02}{s:02}")
}

#[cfg(windows)]
fn local_now() -> [u32; 6] {
    // SAFETY: GetLocalTime only writes the SYSTEMTIME it is handed.
    let t = unsafe {
        let mut t = std::mem::zeroed::<windows_sys::Win32::Foundation::SYSTEMTIME>();
        windows_sys::Win32::System::SystemInformation::GetLocalTime(&mut t);
        t
    };
    [t.wYear, t.wMonth, t.wDay, t.wHour, t.wMinute, t.wSecond].map(u32::from)
}

#[cfg(not(windows))]
fn local_now() -> [u32; 6] {
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
    utc(secs)
}

/// Unix seconds to UTC `[year, month, day, hour, minute, second]` (the civil-from-days
/// algorithm), where there is no local clock to ask.
#[cfg_attr(windows, allow(dead_code))]
fn utc(secs: u64) -> [u32; 6] {
    let days = (secs / 86400) as i64;
    let rem = secs % 86400;
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    [y as u32, m as u32, d as u32, (rem / 3600) as u32, (rem / 60 % 60) as u32, (rem % 60) as u32]
}

/// Copies the saved world into a new backup named `<stamp><suffix>` (`-2`, `-3` added when that
/// name is taken) and returns the name. The copy is written under a `.partial` name and renamed
/// when complete, so a listed backup is always whole.
pub fn create(dir: &Path, suffix: &str) -> io::Result<String> {
    let root = root(dir);
    std::fs::create_dir_all(&root)?;
    let base = format!("{}{suffix}", stamp());
    let mut name = base.clone();
    let mut n = 2;
    while root.join(&name).exists() {
        name = format!("{base}-{n}");
        n += 1;
    }
    let tmp = root.join(format!("{name}{PARTIAL}"));
    if tmp.exists() {
        std::fs::remove_dir_all(&tmp)?;
    }
    std::fs::create_dir_all(&tmp)?;
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let file = entry.file_name();
        let Some(file) = file.to_str() else { continue };
        let kind = entry.file_type()?;
        if kind.is_file() && saved_file(file) {
            std::fs::copy(entry.path(), tmp.join(file))?;
        } else if kind.is_dir() && file == PLAYERDATA {
            copy_players(&entry.path(), &tmp.join(PLAYERDATA))?;
        } else if kind.is_dir() && file == MEDIA {
            let to = tmp.join(MEDIA);
            std::fs::create_dir_all(&to)?;
            for m in std::fs::read_dir(entry.path())? {
                let m = m?;
                if m.file_type()?.is_file() {
                    let dst = to.join(m.file_name());
                    if std::fs::hard_link(m.path(), &dst).is_err() {
                        std::fs::copy(m.path(), &dst)?;
                    }
                }
            }
        }
    }
    std::fs::rename(&tmp, root.join(&name))?;
    Ok(name)
}

/// Copies the `.json` files of a folder into `to`, which is made.
fn copy_players(from: &Path, to: &Path) -> io::Result<()> {
    std::fs::create_dir_all(to)?;
    for f in std::fs::read_dir(from)? {
        let f = f?;
        if f.file_type()?.is_file() && f.file_name().to_str().is_some_and(|n| n.ends_with(".json")) {
            std::fs::copy(f.path(), to.join(f.file_name()))?;
        }
    }
    Ok(())
}

/// The backups, newest first.
pub fn list(dir: &Path) -> Vec<String> {
    let Ok(rd) = std::fs::read_dir(root(dir)) else { return Vec::new() };
    let mut names: Vec<String> = rd
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
        .filter_map(|e| e.file_name().into_string().ok())
        .filter(|n| !n.ends_with(PARTIAL))
        .collect();
    names.sort_unstable_by(|a, b| b.cmp(a));
    names
}

/// Removes every backup past the newest `keep` (0 keeps them all) and any half-written one;
/// returns the names removed.
pub fn prune(dir: &Path, keep: usize) -> io::Result<Vec<String>> {
    let root = root(dir);
    if let Ok(rd) = std::fs::read_dir(&root) {
        for e in rd.flatten() {
            if e.file_name().to_str().is_some_and(|n| n.ends_with(PARTIAL)) {
                std::fs::remove_dir_all(e.path())?;
            }
        }
    }
    if keep == 0 {
        return Ok(Vec::new());
    }
    let old: Vec<String> = list(dir).into_iter().skip(keep).collect();
    for n in &old {
        std::fs::remove_dir_all(root.join(n))?;
    }
    Ok(old)
}

/// Puts the world back as backup `name` left it; the server must not be running on this folder.
/// Backs up the world as it is first and returns that backup's name.
pub fn restore(dir: &Path, name: &str) -> io::Result<String> {
    let from = root(dir).join(name);
    if !valid_name(name) || !from.is_dir() {
        return Err(io::Error::new(io::ErrorKind::NotFound, format!("there is no backup named {name}")));
    }
    let safety = create(dir, "-before-rollback")?;
    // The world files the backup lacks were made after it (a module file, the paintings): gone.
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let file = entry.file_name();
        let Some(file) = file.to_str() else { continue };
        if entry.file_type()?.is_file() && saved_file(file) && !KEPT.contains(&file) {
            std::fs::remove_file(entry.path())?;
        }
    }
    // Players made after the backup have no file in it: gone, like the module files.
    let _ = std::fs::remove_dir_all(dir.join(PLAYERDATA));
    for entry in std::fs::read_dir(&from)? {
        let entry = entry?;
        let file = entry.file_name();
        let Some(file) = file.to_str() else { continue };
        let kind = entry.file_type()?;
        if kind.is_file() && saved_file(file) && !KEPT.contains(&file) {
            std::fs::copy(entry.path(), dir.join(file))?;
        } else if kind.is_dir() && file == PLAYERDATA {
            copy_players(&entry.path(), &dir.join(PLAYERDATA))?;
        } else if kind.is_dir() && file == MEDIA {
            // Pictures are named by their content, so the ones here already match; add the rest.
            let to = dir.join(MEDIA);
            std::fs::create_dir_all(&to)?;
            for m in std::fs::read_dir(entry.path())? {
                let m = m?;
                let dst = to.join(m.file_name());
                if m.file_type()?.is_file() && !dst.exists() {
                    std::fs::copy(m.path(), &dst)?;
                }
            }
        }
    }
    Ok(safety)
}

/// Asks for a rollback to `name` at the next start, or (`None`) cancels one.
pub fn set_pending(dir: &Path, name: Option<&str>) -> io::Result<()> {
    let path = root(dir).join(PENDING);
    match name {
        Some(n) => {
            std::fs::create_dir_all(root(dir))?;
            std::fs::write(path, n)
        }
        None => match std::fs::remove_file(path) {
            Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e),
            _ => Ok(()),
        },
    }
}

/// The rollback asked for at the next start, if any.
pub fn pending(dir: &Path) -> Option<String> {
    let n = std::fs::read_to_string(root(dir).join(PENDING)).ok()?;
    let n = n.trim();
    (!n.is_empty()).then(|| n.to_string())
}

/// At start, before the world loads: the rollback `--rollback` names, else the one `/rollback`
/// asked for (whose request is cleared first, so a failed one does not repeat at every start).
pub fn rollback_at_start(dir: &Path, asked: Option<String>) -> io::Result<Option<(String, String)>> {
    let queued = pending(dir);
    if queued.is_some() {
        set_pending(dir, None)?;
    }
    let Some(name) = asked.or(queued) else { return Ok(None) };
    let safety = restore(dir, &name)?;
    Ok(Some((name, safety)))
}

/// A backup of the running server: the game thread writes its module files, then, with the world
/// held so no block change lands mid-copy, the world writes its own and the folder is copied.
/// Returns the new backup's name and the old ones pruned.
pub fn now(hub: &Hub, suffix: &str) -> io::Result<(String, Vec<String>)> {
    if !hub.save_game(Duration::from_secs(10)) {
        println!("backup: the game thread did not answer in 10 s; backing up its last autosave");
    }
    let name = {
        let mut world = hub.world.lock().unwrap();
        world.save();
        create(&hub.dir, suffix)?
    };
    let pruned = prune(&hub.dir, hub.cfg.backup_keep)?;
    Ok((name, pruned))
}

/// What the backup thread is asked: a backup for this player (`None`: the console).
pub struct Request {
    pub from: Option<u32>,
}

/// The backup thread: `/backup` requests as they come, and every `backup_minutes` (0: never) a
/// backup when someone played since the last one.
pub fn run(hub: Arc<Hub>, rx: Receiver<Request>) {
    let every = Duration::from_secs(u64::from(hub.cfg.backup_minutes) * 60);
    let mut last = Instant::now();
    let mut played = false;
    loop {
        match rx.recv_timeout(Duration::from_secs(30)) {
            Ok(req) => {
                let line = match now(&hub, "") {
                    Ok((name, pruned)) => {
                        last = Instant::now();
                        played = !hub.sessions().is_empty();
                        let gone = if pruned.is_empty() { String::new() } else { format!(" (removed {})", pruned.join(", ")) };
                        Ok(format!("Saved the world to backup {name}{gone}"))
                    }
                    Err(e) => Err(format!("The backup failed: {e}")),
                };
                if req.from.is_some() {
                    match &line {
                        Ok(t) | Err(t) => println!("{t}"),
                    }
                }
                crate::commands::notice(&hub, req.from, line);
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return,
        }
        played |= !hub.sessions().is_empty();
        if !every.is_zero() && played && last.elapsed() >= every {
            last = Instant::now();
            played = false;
            match now(&hub, "") {
                Ok((name, _)) => println!("backup {name} saved"),
                Err(e) => eprintln!("the timed backup failed: {e}"),
            }
        }
    }
}

#[cfg(test)]
#[path = "backup_tests.rs"]
mod tests;
