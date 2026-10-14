//! The shared world's saved state. Terrain comes from the seed, so the save holds only what
//! players changed: `edits.bin` (every changed block, appended as it happens and compacted on
//! start), `players.json` (where each player was, their game mode and spawn point) and
//! `world.json` (the clocks, the world spawn, the game rules and the weather), plus the paintings
//! players hung: `paintings.bin` and the files they show in `media/`, named by content hash.

use std::collections::{BTreeMap, HashMap};
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use proto::{BlockChange, Painting, PlayerState};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq)]
pub struct Saved {
    /// The feet, `None` until the player's session first reported where it stands.
    pub pos: Option<[f64; 3]>,
    pub yaw: f32,
    pub pitch: f32,
    pub flying: bool,
    #[serde(default = "default_game_mode")]
    pub game_mode: u8,
    /// `/spawnpoint`: where `/kill` puts the player instead of the world spawn.
    #[serde(default)]
    pub spawn: Option<[i32; 3]>,
}

/// The game mode a player the server has not seen starts in.
pub const DEFAULT_GAME_MODE: u8 = proto::GM_CREATIVE;

fn default_game_mode() -> u8 {
    DEFAULT_GAME_MODE
}

impl Default for Saved {
    fn default() -> Saved {
        Saved { pos: None, yaw: 0.0, pitch: 0.0, flying: false, game_mode: DEFAULT_GAME_MODE, spawn: None }
    }
}

/// `GameRules`' 1.12.2 rules and defaults. Only `doDaylightCycle` and `sendCommandFeedback`
/// change anything here; the rest are kept so `/gamerule` answers as vanilla does.
pub const DEFAULT_RULES: [(&str, &str); 24] = [
    ("announceAdvancements", "true"),
    ("commandBlockOutput", "true"),
    ("disableElytraMovementCheck", "false"),
    ("doDaylightCycle", "true"),
    ("doEntityDrops", "true"),
    ("doFireTick", "true"),
    ("doLimitedCrafting", "false"),
    ("doMobLoot", "true"),
    ("doMobSpawning", "true"),
    ("doTileDrops", "true"),
    ("doWeatherCycle", "true"),
    ("gameLoopFunction", "-"),
    ("keepInventory", "false"),
    ("logAdminCommands", "true"),
    ("maxCommandChainLength", "65536"),
    ("maxEntityCramming", "24"),
    ("mobGriefing", "true"),
    ("naturalRegeneration", "true"),
    ("randomTickSpeed", "3"),
    ("reducedDebugInfo", "false"),
    ("sendCommandFeedback", "true"),
    ("showDeathMessages", "true"),
    ("spawnRadius", "10"),
    ("spectatorsGenerateChunks", "true"),
];

/// What `/weather` last set. Nothing draws rain yet, so it is only kept.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(default)]
pub struct Weather {
    pub raining: bool,
    pub thundering: bool,
    /// Ticks the weather was set to last.
    pub duration: i32,
}

#[derive(Serialize, Deserialize)]
#[serde(default)]
struct WorldFile {
    /// `WorldInfo.worldTime`, the time of day.
    time: i64,
    /// `WorldInfo.totalWorldTime`, ticks since the world began.
    total: i64,
    spawn: [i32; 3],
    rules: BTreeMap<String, String>,
    weather: Weather,
}

impl Default for WorldFile {
    fn default() -> WorldFile {
        // The viewer's spawn column before the server kept one; respawns stand on its top block.
        WorldFile { time: 0, total: 0, spawn: [8, 64, 8], rules: BTreeMap::new(), weather: Weather::default() }
    }
}

/// The world's two clocks, 20 ticks a second of real time: the time of day, which only the
/// daylight cycle advances, and the total, which always does. Whole ticks are folded in when
/// either changes, so no fraction of a tick is lost or gained.
#[derive(Clone, Copy, Debug)]
pub struct Clock {
    day: i64,
    total: i64,
    cycle: bool,
    since: Instant,
}

impl Clock {
    pub fn new(day: i64, total: i64, cycle: bool, now: Instant) -> Clock {
        Clock { day, total, cycle, since: now }
    }

    fn ticks(&self, now: Instant) -> i64 {
        (now.saturating_duration_since(self.since).as_millis() / 50) as i64
    }

    pub fn day_time(&self, now: Instant) -> i64 {
        self.day + if self.cycle { self.ticks(now) } else { 0 }
    }

    pub fn total_time(&self, now: Instant) -> i64 {
        self.total + self.ticks(now)
    }

    pub fn cycle(&self) -> bool {
        self.cycle
    }

    fn settle(&mut self, now: Instant) {
        let t = self.ticks(now);
        self.day = self.day_time(now);
        self.total += t;
        self.since += Duration::from_millis(t as u64 * 50);
    }

    /// `World.setWorldTime`
    pub fn set_day(&mut self, now: Instant, t: i64) {
        self.settle(now);
        self.day = t;
    }

    pub fn set_cycle(&mut self, now: Instant, on: bool) {
        self.settle(now);
        self.cycle = on;
    }
}

pub struct World {
    dir: PathBuf,
    edits: HashMap<(i32, i32, i32), u16>,
    log: Option<File>,
    players: HashMap<String, Saved>,
    players_dirty: bool,
    pub clock: Clock,
    /// `WorldInfo.spawnX/Y/Z`
    pub spawn: [i32; 3],
    rules: BTreeMap<String, String>,
    pub weather: Weather,
    paintings: BTreeMap<u64, Painting>,
}

/// `paintings.bin`: each painting as a `u16` length and [`Painting::encode`].
fn decode_paintings(raw: &[u8]) -> BTreeMap<u64, Painting> {
    let mut out = BTreeMap::new();
    let mut b = raw;
    while b.len() >= 2 {
        let n = u16::from_le_bytes([b[0], b[1]]) as usize;
        let Some(rec) = b.get(2..2 + n) else { break };
        if let Some(p) = Painting::decode(rec).filter(|p| !p.gone) {
            out.insert(p.uid, p);
        }
        b = &b[2 + n..];
    }
    out
}

impl World {
    pub fn load(dir: &Path) -> std::io::Result<World> {
        std::fs::create_dir_all(dir)?;
        let edits_path = dir.join("edits.bin");
        let raw = std::fs::read(&edits_path).unwrap_or_default();
        let records = raw.len() / 14;
        let mut edits = HashMap::new();
        for (at, s) in proto::decode_blocks(&raw[..records * 14]) {
            edits.insert(at, s);
        }
        // Rewrite the log with one record per block when it has grown past twice that.
        if records > 2 * edits.len() + 1024 || raw.len() % 14 != 0 {
            let list: Vec<BlockChange> = edits.iter().map(|(&a, &s)| (a, s)).collect();
            let tmp = dir.join("edits.bin.tmp");
            std::fs::write(&tmp, proto::encode_blocks(&list))?;
            std::fs::rename(&tmp, &edits_path)?;
        }
        let log = OpenOptions::new().create(true).append(true).open(&edits_path)?;
        let players = std::fs::read_to_string(dir.join("players.json")).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
        let wf: WorldFile = std::fs::read_to_string(dir.join("world.json")).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
        let mut rules = wf.rules;
        for (k, v) in DEFAULT_RULES {
            rules.entry(k.to_string()).or_insert_with(|| v.to_string());
        }
        let cycle = rules.get("doDaylightCycle").is_none_or(|v| v == "true");
        Ok(World {
            dir: dir.to_path_buf(),
            edits,
            log: Some(log),
            players,
            players_dirty: false,
            clock: Clock::new(wf.time, wf.total, cycle, Instant::now()),
            spawn: wf.spawn,
            rules,
            weather: wf.weather,
            paintings: decode_paintings(&std::fs::read(dir.join("paintings.bin")).unwrap_or_default()),
        })
    }

    /// The time of day now.
    pub fn time(&self) -> i64 {
        self.clock.day_time(Instant::now())
    }

    pub fn total_time(&self) -> i64 {
        self.clock.total_time(Instant::now())
    }

    pub fn set_time(&mut self, t: i64) {
        self.clock.set_day(Instant::now(), t);
    }

    /// What the viewers' clocks follow.
    pub fn time_sync(&self, snap: bool) -> proto::TimeSync {
        proto::TimeSync { world_time: self.time(), cycle: self.clock.cycle(), snap }
    }

    pub fn rule(&self, name: &str) -> Option<&str> {
        self.rules.get(name).map(|s| s.as_str())
    }

    pub fn rules(&self) -> impl Iterator<Item = &str> {
        self.rules.keys().map(|k| k.as_str())
    }

    /// `GameRules.setOrCreateGameRule`
    pub fn set_rule(&mut self, name: &str, value: &str) {
        if name == "doDaylightCycle" {
            self.clock.set_cycle(Instant::now(), value == "true");
        }
        self.rules.insert(name.to_string(), value.to_string());
    }

    pub fn edit_count(&self) -> usize {
        self.edits.len()
    }

    /// Every changed block, for a player joining.
    pub fn all_edits(&self) -> Vec<BlockChange> {
        self.edits.iter().map(|(&a, &s)| (a, s)).collect()
    }

    /// Records blocks a player changed. Paintings on a block that became air come down
    /// (`EntityHanging.onValidSurface`); they are returned, for the other players.
    pub fn apply(&mut self, changes: &[BlockChange]) -> Vec<Painting> {
        for &(at, s) in changes {
            self.edits.insert(at, s);
        }
        let air: Vec<[i32; 3]> = changes.iter().filter(|c| c.1 >> 4 == 0).map(|&((x, y, z), _)| [x, y, z]).collect();
        let gone: Vec<u64> = self.paintings.values().filter(|p| air.contains(&p.pos)).map(|p| p.uid).collect();
        let gone: Vec<Painting> = gone.into_iter().map(|uid| Painting { uid, gone: true, ..Default::default() }).collect();
        for p in &gone {
            self.paint(p.clone());
        }
        if let Some(log) = &mut self.log {
            if log.write_all(&proto::encode_blocks(changes)).is_err() {
                eprintln!("could not append to edits.bin; edits stay in memory until the next save works");
                self.log = None;
            }
        }
        gone
    }

    /// Where the media files are kept.
    fn media_path(&self, id: &str) -> Option<PathBuf> {
        proto::valid_media_id(id).then(|| self.dir.join("media").join(id))
    }

    /// Keeps a media file under its content name; false when the name does not match the bytes.
    pub fn store_media(&self, id: &str, bytes: &[u8]) -> bool {
        let ext = id.split_once('.').map_or("", |e| e.1);
        let Some(path) = self.media_path(id).filter(|_| proto::media_id(bytes, ext) == id && bytes.len() <= proto::MAX_MEDIA) else { return false };
        if path.exists() {
            return true;
        }
        let _ = std::fs::create_dir_all(path.parent().unwrap());
        let tmp = path.with_extension("tmp");
        std::fs::write(&tmp, bytes).and_then(|_| std::fs::rename(&tmp, &path)).is_ok()
    }

    pub fn media(&self, id: &str) -> Option<Vec<u8>> {
        std::fs::read(self.media_path(id)?).ok()
    }

    pub fn paintings(&self) -> Vec<Painting> {
        self.paintings.values().cloned().collect()
    }

    /// Hangs, resizes or takes down a painting and saves the list. False when nothing changed or
    /// the painting shows a file the server does not have.
    pub fn paint(&mut self, p: Painting) -> bool {
        let changed = if p.gone {
            self.paintings.remove(&p.uid).is_some()
        } else if self.media_path(&p.media).is_some_and(|m| m.exists()) && self.paintings.get(&p.uid) != Some(&p) {
            self.paintings.insert(p.uid, p);
            true
        } else {
            false
        };
        if changed {
            let mut raw = Vec::new();
            for p in self.paintings.values() {
                let rec = p.encode();
                raw.extend_from_slice(&(rec.len() as u16).to_le_bytes());
                raw.extend_from_slice(&rec);
            }
            let path = self.dir.join("paintings.bin");
            let tmp = path.with_extension("tmp");
            if std::fs::write(&tmp, raw).is_ok() {
                let _ = std::fs::rename(&tmp, path);
            }
        }
        changed
    }

    pub fn player(&self, name: &str) -> Option<Saved> {
        self.players.get(&name.to_ascii_lowercase()).copied()
    }

    /// Changes what is kept about a player, creating the record when there is none.
    pub fn update_player(&mut self, name: &str, f: impl FnOnce(&mut Saved)) {
        let key = name.to_ascii_lowercase();
        let new = !self.players.contains_key(&key);
        let entry = self.players.entry(key).or_default();
        let before = *entry;
        f(entry);
        if new || *entry != before {
            self.players_dirty = true;
        }
    }

    pub fn game_mode(&self, name: &str) -> u8 {
        self.player(name).map_or(DEFAULT_GAME_MODE, |s| s.game_mode)
    }

    pub fn remember(&mut self, name: &str, s: &PlayerState) {
        self.update_player(name, |saved| {
            saved.pos = Some(s.pos);
            saved.yaw = s.yaw;
            saved.pitch = s.pitch;
            saved.flying = s.flags & proto::FLYING != 0;
        });
    }

    /// The data folder the world is saved in.
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Writes the players and the world file; called every few seconds and at shutdown.
    pub fn save(&mut self) {
        if self.players_dirty {
            self.players_dirty = false;
            write_atomic(&self.dir.join("players.json"), &serde_json::to_string_pretty(&self.players).unwrap());
        }
        let now = Instant::now();
        let wf = WorldFile {
            time: self.clock.day_time(now),
            total: self.clock.total_time(now),
            spawn: self.spawn,
            rules: self.rules.clone(),
            weather: self.weather.clone(),
        };
        write_atomic(&self.dir.join("world.json"), &serde_json::to_string_pretty(&wf).unwrap());
        if let Some(log) = &mut self.log {
            let _ = log.flush();
        }
    }
}

pub(crate) fn write_atomic(path: &Path, text: &str) {
    let tmp = path.with_extension("tmp");
    if std::fs::write(&tmp, text).is_ok() {
        let _ = std::fs::rename(&tmp, path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edits_and_players_survive_a_restart_and_the_log_compacts() {
        let d = std::env::temp_dir().join(format!("mcrs-world-{}", crate::config::random_u64()));
        {
            let mut w = World::load(&d).unwrap();
            for i in 0..3000 {
                w.apply(&[((1, 64, 1), (i % 2) as u16 * 16)]);
            }
            w.apply(&[((-5, 70, 9), 32)]);
            w.remember("Xy", &PlayerState { pos: [1.0, 65.0, 2.0], yaw: 45.0, flags: proto::FLYING, ..Default::default() });
            w.update_player("xY", |s| s.spawn = Some([3, 70, -4]));
            w.update_player("other", |s| s.game_mode = proto::GM_SPECTATOR);
            w.spawn = [100, 70, -20];
            w.set_rule("doDaylightCycle", "false");
            w.set_time(18000);
            w.save();
        }
        let w = World::load(&d).unwrap();
        assert_eq!(w.edit_count(), 2);
        let mut all = w.all_edits();
        all.sort();
        assert_eq!(all, vec![((-5, 70, 9), 32), ((1, 64, 1), 16)]);
        assert_eq!(std::fs::metadata(d.join("edits.bin")).unwrap().len(), 28, "compacted to one record a block");
        assert_eq!(
            w.player("xy"),
            Some(Saved { pos: Some([1.0, 65.0, 2.0]), yaw: 45.0, pitch: 0.0, flying: true, game_mode: DEFAULT_GAME_MODE, spawn: Some([3, 70, -4]) })
        );
        assert_eq!(w.game_mode("Other"), proto::GM_SPECTATOR);
        assert_eq!(w.game_mode("nobody"), DEFAULT_GAME_MODE);
        assert_eq!(w.spawn, [100, 70, -20]);
        assert_eq!(w.rule("doDaylightCycle"), Some("false"));
        assert_eq!(w.rule("keepInventory"), Some("false"));
        assert_eq!(w.time(), 18000, "the cycle is off, so the time stands");
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn paintings_and_their_media_are_kept_and_come_down_with_their_block() {
        let d = std::env::temp_dir().join(format!("mcrs-world-{}", crate::config::random_u64()));
        let id = proto::media_id(b"png bytes", "png");
        let p = Painting { uid: 7, pos: [1, 64, 1], face: 5, up: 1, w: 2, h: 1, gone: false, media: id.clone() };
        {
            let mut w = World::load(&d).unwrap();
            assert!(!w.paint(p.clone()), "no media yet");
            assert!(!w.store_media(&id, b"other bytes"), "the name is the hash");
            assert!(!w.store_media("../evil.png", b"png bytes"));
            assert!(w.store_media(&id, b"png bytes"));
            assert!(w.paint(p.clone()));
            assert!(!w.paint(p.clone()), "unchanged");
            assert!(w.paint(Painting { uid: 8, pos: [5, 64, 5], ..p.clone() }));
        }
        let mut w = World::load(&d).unwrap();
        assert_eq!(w.paintings().len(), 2);
        assert_eq!(w.media(&id).as_deref(), Some(&b"png bytes"[..]));
        let gone = w.apply(&[((1, 64, 1), 0), ((5, 65, 5), 0)]);
        assert_eq!(gone, vec![Painting { uid: 7, gone: true, ..Default::default() }]);
        assert_eq!(World::load(&d).unwrap().paintings().iter().map(|p| p.uid).collect::<Vec<_>>(), vec![8]);
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn an_old_save_still_loads() {
        let d = std::env::temp_dir().join(format!("mcrs-world-{}", crate::config::random_u64()));
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("world.json"), r#"{"time":6000}"#).unwrap();
        std::fs::write(d.join("players.json"), r#"{"xy":{"pos":[1.0,65.0,2.0],"yaw":0.0,"pitch":0.0,"flying":false}}"#).unwrap();
        let w = World::load(&d).unwrap();
        assert!((6000..6100).contains(&w.time()));
        assert_eq!(w.spawn, [8, 64, 8]);
        assert_eq!(w.player("xy").map(|s| (s.pos, s.game_mode, s.spawn)), Some((Some([1.0, 65.0, 2.0]), DEFAULT_GAME_MODE, None)));
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn the_clock_ticks_twenty_a_second_and_stops_with_the_cycle() {
        let t0 = Instant::now();
        let at = |ms: u64| t0 + Duration::from_millis(ms);
        let mut c = Clock::new(1000, 50, true, t0);
        assert_eq!((c.day_time(at(0)), c.total_time(at(0))), (1000, 50));
        assert_eq!(c.day_time(at(49)), 1000);
        assert_eq!(c.day_time(at(50)), 1001);
        assert_eq!((c.day_time(at(1000)), c.total_time(at(1000))), (1020, 70));
        // Off at 1.025 s: the day stops, the total keeps going, and the half tick carries over.
        c.set_cycle(at(1025), false);
        assert_eq!(c.day_time(at(5000)), 1020);
        assert_eq!(c.total_time(at(5000)), 150);
        c.set_day(at(5010), 13000);
        assert_eq!(c.day_time(at(9000)), 13000);
        c.set_cycle(at(9000), true);
        assert_eq!(c.day_time(at(10000)), 13020);
        assert_eq!(c.total_time(at(10000)), 250);
    }
}
