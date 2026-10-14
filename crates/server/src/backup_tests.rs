//! Tests for backup.rs: what a backup holds, retention, rollback and the request at start.

use super::*;

fn temp_dir() -> PathBuf {
    let d = std::env::temp_dir().join(format!("mcrs-backup-{}", crate::config::random_u64()));
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn put(dir: &Path, file: &str, text: &str) {
    if let Some(p) = Path::new(file).parent() {
        std::fs::create_dir_all(dir.join(p)).unwrap();
    }
    std::fs::write(dir.join(file), text).unwrap();
}

fn read(dir: &Path, file: &str) -> Option<String> {
    std::fs::read_to_string(dir.join(file)).ok()
}

/// A data folder as a running server leaves it.
fn world() -> PathBuf {
    let d = temp_dir();
    for (f, t) in [
        ("edits.bin", "edits-1"),
        ("players.json", "players-1"),
        ("world.json", "world-1"),
        ("tiles.json", "tiles-1"),
        ("accounts.json", "accounts-1"),
        ("server.json", "server-1"),
        ("latency.log", "log"),
        ("tunnel.log", "log"),
        ("media/aa11.png", "picture"),
    ] {
        put(&d, f, t);
    }
    d
}

#[test]
fn a_backup_holds_the_world_and_settings_but_not_the_logs() {
    let d = world();
    let name = create(&d, "").unwrap();
    assert_eq!(name.len(), 15);
    let b = d.join(DIR).join(&name);
    for (f, t) in [("edits.bin", "edits-1"), ("players.json", "players-1"), ("world.json", "world-1"), ("tiles.json", "tiles-1"), ("accounts.json", "accounts-1"), ("server.json", "server-1"), ("media/aa11.png", "picture")] {
        assert_eq!(read(&b, f).as_deref(), Some(t), "{f}");
    }
    assert!(read(&b, "latency.log").is_none() && read(&b, "tunnel.log").is_none());
    // The backups folder itself is never copied into the next backup.
    let second = create(&d, "").unwrap();
    assert_ne!(second, name);
    assert!(!d.join(DIR).join(&second).join(DIR).exists());
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn names_never_collide_and_list_newest_first() {
    let d = world();
    let a = create(&d, "").unwrap();
    let b = create(&d, "").unwrap();
    let c = create(&d, "").unwrap();
    let all = list(&d);
    assert_eq!(all.len(), 3);
    assert!(all.contains(&a) && all.contains(&b) && all.contains(&c));
    let mut sorted = all.clone();
    sorted.sort_unstable_by(|x, y| y.cmp(x));
    assert_eq!(all, sorted);
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn prune_keeps_the_newest_and_clears_half_written_ones() {
    let d = world();
    for n in ["20260101-000000", "20260102-000000", "20260103-000000", "20260104-000000"] {
        std::fs::create_dir_all(d.join(DIR).join(n)).unwrap();
    }
    std::fs::create_dir_all(d.join(DIR).join("20260105-000000.partial")).unwrap();
    assert_eq!(list(&d).len(), 4, "a partial backup is never listed");
    assert_eq!(prune(&d, 0).unwrap(), Vec::<String>::new());
    assert_eq!(list(&d).len(), 4, "keep 0 keeps every one");
    assert!(!d.join(DIR).join("20260105-000000.partial").exists());
    assert_eq!(prune(&d, 2).unwrap(), vec!["20260102-000000".to_string(), "20260101-000000".to_string()]);
    assert_eq!(list(&d), vec!["20260104-000000".to_string(), "20260103-000000".to_string()]);
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn restore_puts_the_world_back_and_keeps_accounts_and_settings() {
    let d = world();
    let name = create(&d, "").unwrap();
    // Play goes on after the backup.
    put(&d, "edits.bin", "edits-2");
    put(&d, "players.json", "players-2");
    put(&d, "mobs.json", "mobs-2");
    put(&d, "paintings.bin", "paintings-2");
    put(&d, "accounts.json", "accounts-2");
    put(&d, "server.json", "server-2");
    put(&d, "media/bb22.png", "picture-2");
    std::fs::remove_file(d.join("media/aa11.png")).unwrap();
    let safety = restore(&d, &name).unwrap();
    assert!(safety.ends_with("-before-rollback"));
    assert_eq!(read(&d, "edits.bin").as_deref(), Some("edits-1"));
    assert_eq!(read(&d, "players.json").as_deref(), Some("players-1"));
    assert_eq!(read(&d, "tiles.json").as_deref(), Some("tiles-1"));
    assert!(read(&d, "mobs.json").is_none() && read(&d, "paintings.bin").is_none(), "files made after the backup go");
    assert_eq!(read(&d, "accounts.json").as_deref(), Some("accounts-2"));
    assert_eq!(read(&d, "server.json").as_deref(), Some("server-2"));
    assert_eq!(read(&d, "media/aa11.png").as_deref(), Some("picture"));
    assert_eq!(read(&d, "media/bb22.png").as_deref(), Some("picture-2"));
    assert_eq!(read(&d, "latency.log").as_deref(), Some("log"));
    // The world as it was before the rollback is a backup of its own, so the rollback can be undone.
    let s = d.join(DIR).join(&safety);
    assert_eq!(read(&s, "edits.bin").as_deref(), Some("edits-2"));
    assert_eq!(read(&s, "mobs.json").as_deref(), Some("mobs-2"));
    restore(&d, &safety).unwrap();
    assert_eq!(read(&d, "edits.bin").as_deref(), Some("edits-2"));
    assert_eq!(read(&d, "mobs.json").as_deref(), Some("mobs-2"));
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn restore_refuses_names_outside_the_backups() {
    let d = world();
    create(&d, "").unwrap();
    for bad in ["", ".", "..", "../x", "..\\x", "C:", "nope", "20260101-000000.partial"] {
        assert!(restore(&d, bad).is_err(), "{bad:?}");
    }
    assert_eq!(read(&d, "edits.bin").as_deref(), Some("edits-1"));
    assert_eq!(list(&d).len(), 1, "a refused rollback makes no backup");
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn a_requested_rollback_runs_once_at_start() {
    let d = world();
    let name = create(&d, "").unwrap();
    assert_eq!(rollback_at_start(&d, None).unwrap(), None);
    put(&d, "edits.bin", "edits-2");
    set_pending(&d, Some(&name)).unwrap();
    assert_eq!(pending(&d).as_deref(), Some(name.as_str()));
    let (rolled, _) = rollback_at_start(&d, None).unwrap().unwrap();
    assert_eq!(rolled, name);
    assert_eq!(read(&d, "edits.bin").as_deref(), Some("edits-1"));
    assert_eq!(pending(&d), None);
    assert_eq!(rollback_at_start(&d, None).unwrap(), None);
    // A request for a backup that is gone fails once, and the next start runs normally.
    set_pending(&d, Some("19990101-000000")).unwrap();
    assert!(rollback_at_start(&d, None).is_err());
    assert_eq!(rollback_at_start(&d, None).unwrap(), None);
    // The command line's name wins over a request.
    put(&d, "edits.bin", "edits-3");
    set_pending(&d, Some("19990101-000000")).unwrap();
    assert_eq!(rollback_at_start(&d, Some(name.clone())).unwrap().unwrap().0, name);
    assert_eq!(read(&d, "edits.bin").as_deref(), Some("edits-1"));
    set_pending(&d, None).unwrap();
    set_pending(&d, None).unwrap();
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn utc_dates_and_the_stamp_shape() {
    assert_eq!(utc(0), [1970, 1, 1, 0, 0, 0]);
    assert_eq!(utc(1_760_000_000), [2025, 10, 9, 8, 53, 20]);
    assert_eq!(utc(951_782_400), [2000, 2, 29, 0, 0, 0]);
    assert_eq!(utc(4_107_542_399), [2100, 2, 28, 23, 59, 59]);
    let s = stamp();
    assert_eq!(s.len(), 15);
    assert!(s.chars().enumerate().all(|(i, c)| if i == 8 { c == '-' } else { c.is_ascii_digit() }), "{s}");
}
