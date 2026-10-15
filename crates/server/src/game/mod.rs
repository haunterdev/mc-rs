//! The game: one simulation on its own thread, named
//! `game`, ticking 20 times a second. It owns what more than one player sees or what outlives a
//! session (health, inventories, item entities, mobs, chests and furnaces). The hub sends it
//! [`GameMsg`] values; it writes frames into each session's `to_viewer` sender (net.rs).
//!
//! Behaviour lives in the hook modules, each filled by one task of section 5. Every module has the
//! same six hooks (`on_join`, `on_leave`, `on_frame`, `tick`, `save`, `load`), listed once in
//! [`MODULES`]; the tick runs [`TICK_ORDER`]. A few messages go to one module's own hook:
//! `State` to `players::on_state`, `Blocks` to `terrain::on_blocks`, `mining::on_blocks` and
//! `tiles::on_changes`,
//! `GameMode` to `players::on_game_mode`, `Kill` to `damage::on_kill`, and the start and
//! `Shutdown` to `persist`. A module keeps its data in its own `State`, a field of [`Game`].
//!
//! Every 5 s (100 ticks) the game appends a `source: server`, `event: tick` line to latency.log:
//! tick time p50, p99 and max, and the counts in [`TickStats`] the modules keep up to date.

// The hooks and helpers are what the later tasks call; most are empty until then.
#![allow(dead_code)]

pub mod addons;
pub mod armor;
pub mod beds;
pub mod doors;
pub mod effects;
pub mod blockticks;
pub mod damage;
pub mod explosion;
pub mod farming;
pub mod food;
pub mod items;
pub mod jukebox;
pub mod mining;
pub mod mobs;
pub mod net;
pub mod persist;
pub mod playerdata;
pub mod players;
pub mod terrain;
pub mod tiles;
pub mod tracker;
pub mod use_item;
pub mod use_on;
pub mod windows;
pub mod xp;
pub mod falling_blocks;
pub mod xp_orbs;

#[cfg(test)]
pub mod testing;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod players_tests;
#[cfg(test)]
mod tiles_tests;
#[cfg(test)]
mod tiles_t8_tests;
#[cfg(test)]
mod use_on_tests;
#[cfg(test)]
mod farming_tests;
#[cfg(test)]
mod armor_tests;
#[cfg(test)]
mod use_item_tests;
#[cfg(test)]
mod beds_tests;
#[cfg(test)]
mod doors_tests;
#[cfg(test)]
mod progression_tests;
#[cfg(test)]
mod xp_tests;
#[cfg(test)]
mod falling_blocks_tests;
#[cfg(test)]
mod enchant_tests;
#[cfg(test)]
mod effects_tests;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::sync::mpsc::{Receiver, Sender, TryRecvError};
use std::time::{Duration, Instant};

use proto::{BlockChange, PlayerState};
use serde_json::{json, Value};
use tokio::sync::mpsc::UnboundedSender;

use crate::config::Config;
use crate::world::World;

/// One game tick, 20 a second.
pub const TICK: Duration = Duration::from_millis(50);
/// How far behind the loop catches up, running ticks back to back; past this the backlog is
/// dropped (`MinecraftServer.run` caps the lag at 2000 ms the same way).
pub const MAX_BEHIND: Duration = Duration::from_millis(2000);
/// Ticks between two tick lines in latency.log: 5 s.
pub const LOG_EVERY: u64 = 100;

/// What the hub tells the game.
pub enum GameMsg {
    /// A session joined. `survival_client` turns true when its `V_CLIENT_INFO` arrives.
    Join { id: u32, name: String, to_viewer: UnboundedSender<Vec<u8>>, mode: u8, survival_client: bool },
    Leave { id: u32 },
    /// A survival frame from the session, tags 60..99.
    Frame { id: u32, tag: u8, body: Vec<u8> },
    State { id: u32, state: PlayerState },
    /// The block changes the session made, as the hub relayed them.
    Blocks { id: u32, changes: Vec<BlockChange> },
    GameMode { id: u32, mode: u8 },
    Kill { id: u32 },
    /// `/effect` on a player.
    Potion { id: u32, op: crate::commands::PotionOp },
    /// `/summon`: a mob of this kind at this position.
    Summon { kind: &'static sim::kinds::Kind, pos: [f64; 3] },
    /// `/difficulty`: the id 0 to 3.
    Difficulty { id: u8 },
    /// A command moved the player (`/tp`, `/spawn`).
    Teleport { id: u32 },
    /// Write every module's file now and answer on `done` (a backup is about to copy them).
    Save { done: std::sync::mpsc::Sender<()> },
    Shutdown,
}

/// A session as the game knows it.
pub struct Session {
    pub name: String,
    pub to_viewer: UnboundedSender<Vec<u8>>,
    pub mode: u8,
    pub survival_client: bool,
    /// The last state the session sent.
    pub state: Option<PlayerState>,
}

/// The numbers in the tick line. The modules keep the gauges current; the counters run over one
/// line's 100 ticks.
#[derive(Default, Debug)]
pub struct TickStats {
    pub entities: usize,
    pub items: usize,
    pub mobs: usize,
    pub loaded_chunks: usize,
    pub pathfinding_us: u64,
    /// Digs where the viewer's `seen_state` differed from the server's (section 1.3).
    pub dig_mismatches: u64,
    pub frames: u64,
    /// Frames no module took.
    pub frames_unhandled: u64,
    /// Unhandled frames by tag, so the tick line names what no module took.
    pub unhandled_tags: std::collections::BTreeMap<u8, u64>,
    /// Ticks dropped because the loop fell more than [`MAX_BEHIND`] behind.
    pub dropped_ticks: u64,
}

/// One hook module: the six hooks every module implements.
pub struct Module {
    pub name: &'static str,
    pub on_join: fn(&mut Game, u32),
    pub on_leave: fn(&mut Game, u32),
    pub on_frame: fn(&mut Game, u32, u8, &[u8]) -> bool,
    pub tick: fn(&mut Game),
    pub save: fn(&Game) -> Value,
    pub load: fn(&mut Game, Value),
}

macro_rules! module {
    ($m:ident) => {
        Module {
            name: stringify!($m),
            on_join: $m::on_join,
            on_leave: $m::on_leave,
            on_frame: $m::on_frame,
            tick: $m::tick,
            save: $m::save,
            load: $m::load,
        }
    };
}

/// Every hook module. Joins and frames go through them in this order (a frame stops at the first
/// that takes it), leaves in the reverse order, so a player's data loads before the modules that
/// read it and saves after them.
pub const MODULES: &[Module] = &[
    module!(persist),
    module!(playerdata),
    module!(terrain),
    module!(players),
    module!(damage),
    module!(mining),
    module!(blockticks),
    module!(windows),
    module!(use_on),
    module!(use_item),
    module!(beds),
    module!(items),
    module!(mobs),
    module!(tiles),
    module!(tracker),
];

/// One tick, in section 1.3's order after the messages are drained: terrain, players (survival
/// state, damage, digging, open windows), item entities, mobs, spawning, tile entities, tracker,
/// autosave.
pub const TICK_ORDER: &[(&str, fn(&mut Game))] = &[
    ("terrain", terrain::tick),
    ("blockticks", blockticks::tick),
    ("players", players::tick),
    ("damage", damage::tick),
    ("mining", mining::tick),
    ("use_item", use_item::tick),
    ("windows", windows::tick),
    ("beds", beds::tick),
    ("items", items::tick),
    ("falling_blocks", falling_blocks::tick),
    ("xp_orbs", xp_orbs::tick),
    ("mobs", mobs::tick),
    ("spawning", mobs::spawn_step),
    ("tiles", tiles::tick),
    ("tracker", tracker::tick),
    ("playerdata", playerdata::tick),
    ("persist", persist::tick),
];

pub struct Game {
    pub cfg: Config,
    /// The data folder, where the save files go.
    pub dir: PathBuf,
    pub sessions: HashMap<u32, Session>,
    /// Ticks run since the game started.
    pub ticks: u64,
    /// The next server entity id (net::new_entity_id).
    pub next_entity: u32,
    pub stats: TickStats,
    pub terrain: terrain::State,
    pub players: players::State,
    pub damage: damage::State,
    pub playerdata: playerdata::State,
    pub items: items::State,
    pub xp_orbs: xp_orbs::State,
    pub falling: falling_blocks::State,
    pub mining: mining::State,
    pub blockticks: blockticks::State,
    pub windows: windows::State,
    pub beds: beds::State,
    pub use_item: use_item::State,
    pub tiles: tiles::State,
    pub mobs: mobs::State,
    /// The addons switched on and their settings (`addons/`).
    pub addons: addons::State,
    pub tracker: tracker::State,
    pub persist: persist::State,
    /// Set by `Shutdown`, or when every sender is gone.
    pub stopping: bool,
    rx: Receiver<GameMsg>,
    log: Box<dyn FnMut(Value) + Send>,
    /// Tick times in ms since the last tick line.
    tick_ms: Vec<f32>,
}

/// Starts the game thread, its terrain over the hub's `world`. `log` appends a line to latency.log
/// (the hub's writer).
pub fn start(cfg: Config, dir: PathBuf, world: Arc<Mutex<World>>, log: Box<dyn FnMut(Value) + Send>) -> std::io::Result<(Sender<GameMsg>, std::thread::JoinHandle<Game>)> {
    let (tx, rx) = std::sync::mpsc::channel();
    // Population runs on this thread and recurses deeply (terrain.rs).
    let thread = std::thread::Builder::new().name("game".into()).stack_size(terrain::STACK).spawn(move || {
        let mut g = Game::new(cfg, dir, rx, log);
        terrain::attach(&mut g, world);
        g.run()
    })?;
    Ok((tx, thread))
}

/// What the loop does at `now` with the next tick due at `next`: `Ok` with the deadline after
/// this tick, or `Err` with how long to wait. Deadlines are absolute, so a late tick is followed
/// by the next one at once until the loop has caught up; more than [`MAX_BEHIND`] late, the
/// backlog is dropped and the schedule restarts from now.
pub fn step(now: Instant, next: Instant) -> Result<Instant, Duration> {
    if now < next {
        return Err(next - now);
    }
    if now - next > MAX_BEHIND {
        return Ok(now + TICK);
    }
    Ok(next + TICK)
}

/// The value at fraction `p` (0 to 1) of a sorted list, nearest rank.
fn percentile(sorted: &[f32], p: f32) -> f32 {
    if sorted.is_empty() {
        return 0.0;
    }
    sorted[((sorted.len() - 1) as f32 * p).round() as usize]
}

impl Game {
    pub fn new(cfg: Config, dir: PathBuf, rx: Receiver<GameMsg>, log: Box<dyn FnMut(Value) + Send>) -> Game {
        let mut g = Game {
            cfg,
            dir,
            sessions: HashMap::new(),
            ticks: 0,
            next_entity: net::FIRST_ENTITY_ID,
            stats: TickStats::default(),
            terrain: Default::default(),
            players: Default::default(),
            damage: Default::default(),
            playerdata: Default::default(),
            items: Default::default(),
            xp_orbs: Default::default(),
            falling: Default::default(),
            mining: Default::default(),
            blockticks: Default::default(),
            windows: Default::default(),
            beds: Default::default(),
            use_item: Default::default(),
            tiles: Default::default(),
            mobs: Default::default(),
            addons: Default::default(),
            tracker: Default::default(),
            persist: Default::default(),
            stopping: false,
            rx,
            log,
            tick_ms: Vec::with_capacity(LOG_EVERY as usize),
        };
        // Before the saved mobs load: `mobs::spawn` runs the addons' hooks on each.
        g.addons = addons::State::load(&g.cfg, &g.dir);
        persist::on_start(&mut g);
        g
    }

    /// The tick loop, until `Shutdown`. Returns the game for tests.
    pub fn run(mut self) -> Game {
        let mut next = Instant::now();
        while !self.stopping {
            let now = Instant::now();
            match step(now, next) {
                Err(wait) => std::thread::sleep(wait),
                Ok(after) => {
                    if now - next > MAX_BEHIND {
                        let dropped = ((now - next).as_millis() / TICK.as_millis()) as u64;
                        self.stats.dropped_ticks += dropped;
                        println!("game: {} ms behind, {dropped} ticks dropped", (now - next).as_millis());
                    }
                    next = after;
                    self.tick();
                }
            }
        }
        self
    }

    /// One tick: the messages, then every step of [`TICK_ORDER`], then the tick line when due.
    pub fn tick(&mut self) {
        let start = Instant::now();
        self.drain();
        if self.stopping {
            return;
        }
        for (_, step) in TICK_ORDER {
            step(self);
        }
        self.ticks += 1;
        self.tick_ms.push(start.elapsed().as_secs_f32() * 1000.0);
        if self.ticks % LOG_EVERY == 0 {
            self.log_ticks();
        }
    }

    fn drain(&mut self) {
        loop {
            match self.rx.try_recv() {
                Ok(m) => self.handle(m),
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    self.shutdown();
                    break;
                }
            }
            if self.stopping {
                break;
            }
        }
    }

    pub fn handle(&mut self, m: GameMsg) {
        match m {
            GameMsg::Join { id, name, to_viewer, mode, survival_client } => {
                self.sessions.insert(id, Session { name, to_viewer, mode, survival_client, state: None });
                for m in MODULES {
                    (m.on_join)(self, id);
                }
            }
            GameMsg::Leave { id } => {
                if !self.sessions.contains_key(&id) {
                    return;
                }
                for m in MODULES.iter().rev() {
                    (m.on_leave)(self, id);
                }
                self.sessions.remove(&id);
            }
            GameMsg::Frame { id, tag, body } => {
                let Some(s) = self.sessions.get_mut(&id) else { return };
                self.stats.frames += 1;
                let mut taken = false;
                if tag == proto::V_CLIENT_INFO {
                    if let Some(info) = proto::ClientInfo::decode(&body) {
                        s.survival_client = info.version >= proto::SURVIVAL_PROTOCOL;
                        taken = true;
                    }
                }
                for m in MODULES {
                    if (m.on_frame)(self, id, tag, &body) {
                        taken = true;
                        break;
                    }
                }
                if !taken {
                    self.stats.frames_unhandled += 1;
                    *self.stats.unhandled_tags.entry(tag).or_default() += 1;
                }
            }
            GameMsg::State { id, state } => {
                if !self.sessions.contains_key(&id) {
                    return;
                }
                players::on_state(self, id, &state);
                if let Some(s) = self.sessions.get_mut(&id) {
                    s.state = Some(state);
                }
            }
            GameMsg::Blocks { id, changes } => {
                if !self.sessions.contains_key(&id) {
                    return;
                }
                mining::before_blocks(self, &changes);
                terrain::on_blocks(self, id, &changes);
                mining::on_blocks(self, id, &changes);
                tiles::on_changes(self, Some(id), &changes);
            }
            GameMsg::Difficulty { id } => {
                self.cfg.difficulty = ["peaceful", "easy", "normal", "hard"][id.min(3) as usize].into();
            }
            GameMsg::GameMode { id, mode } => {
                let Some(s) = self.sessions.get_mut(&id) else { return };
                s.mode = mode;
                players::on_game_mode(self, id, mode);
            }
            GameMsg::Potion { id, op } => {
                if self.players.players.contains_key(&id) {
                    match op {
                        crate::commands::PotionOp::Add(e) => effects::add(self, id, e),
                        crate::commands::PotionOp::Remove(e) => effects::remove(self, id, e),
                        crate::commands::PotionOp::Clear => {
                            effects::clear(self, id);
                        }
                    }
                }
            }
            GameMsg::Kill { id } => {
                if self.sessions.contains_key(&id) {
                    damage::on_kill(self, id);
                }
            }
            GameMsg::Summon { kind, pos } => {
                mobs::summon(self, kind, pos);
            }
            GameMsg::Teleport { id } => {
                if self.sessions.contains_key(&id) {
                    players::on_teleport(self, id);
                }
            }
            GameMsg::Save { done } => {
                persist::save_all(self);
                let _ = done.send(());
            }
            GameMsg::Shutdown => self.shutdown(),
        }
    }

    fn shutdown(&mut self) {
        if !self.stopping {
            self.stopping = true;
            persist::on_shutdown(self);
        }
    }

    /// The tick line: tick time p50, p99 and max over the last [`LOG_EVERY`] ticks, and the stats.
    fn log_ticks(&mut self) {
        let mut ms = std::mem::take(&mut self.tick_ms);
        ms.sort_by(|a, b| a.total_cmp(b));
        let round = |v: f32| (v as f64 * 1000.0).round() / 1000.0;
        let s = &self.stats;
        let line = json!({
            "event": "tick",
            "ticks": ms.len(),
            "tick_p50_ms": round(percentile(&ms, 0.5)),
            "tick_p99_ms": round(percentile(&ms, 0.99)),
            "tick_max_ms": round(ms.last().copied().unwrap_or(0.0)),
            "players": self.sessions.len(),
            "entities": s.entities,
            "items": s.items,
            "mobs": s.mobs,
            "loaded_chunks": s.loaded_chunks,
            "pathfinding_ms": s.pathfinding_us as f64 / 1000.0,
            "dig_mismatches": s.dig_mismatches,
            "frames": s.frames,
            "frames_unhandled": s.frames_unhandled,
            "unhandled_tags": s.unhandled_tags.iter().map(|(t, n)| (t.to_string(), *n)).collect::<std::collections::BTreeMap<_, _>>(),
            "dropped_ticks": s.dropped_ticks,
        });
        (self.log)(line);
        self.stats.pathfinding_us = 0;
        self.stats.dig_mismatches = 0;
        self.stats.frames = 0;
        self.stats.frames_unhandled = 0;
        self.stats.unhandled_tags.clear();
        self.stats.dropped_ticks = 0;
        ms.clear();
        self.tick_ms = ms;
    }
}
