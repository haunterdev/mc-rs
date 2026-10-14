//! A game for tests: seeded, with its terrain over a world in a temporary folder, and fake
//! sessions that collect every frame the game sends them.
//!
//! Population recurses deeply, so a test that ticks terrain runs inside [`run`], on a thread with
//! the game thread's stack.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::mpsc::{channel, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use proto::PlayerState;
use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver};

use super::{terrain, Game, GameMsg};
use crate::config::Config;
use crate::world::World;

pub struct Harness {
    pub game: Game,
    pub world: Arc<Mutex<World>>,
    pub dir: PathBuf,
    tx: Sender<GameMsg>,
    inboxes: HashMap<u32, UnboundedReceiver<Vec<u8>>>,
    frames: HashMap<u32, Vec<(u8, Vec<u8>)>>,
}

/// A fresh folder under the system temp folder.
pub fn temp_dir() -> PathBuf {
    std::env::temp_dir().join(format!("mcrs-harness-{}", crate::config::random_u64()))
}

/// Runs `f` on a thread with the game thread's stack and passes on its panic.
pub fn run(f: impl FnOnce() + Send + 'static) {
    let t = std::thread::Builder::new().stack_size(terrain::STACK).spawn(f).unwrap();
    if let Err(e) = t.join() {
        std::panic::resume_unwind(e);
    }
}

impl Harness {
    /// A game for `seed` that keeps `sim_distance` chunks around each player, in a new folder.
    pub fn new(seed: i64, sim_distance: i32) -> Harness {
        Harness::in_dir(seed, sim_distance, temp_dir())
    }

    /// The same over the world already saved in `dir`.
    pub fn in_dir(seed: i64, sim_distance: i32, dir: PathBuf) -> Harness {
        let cfg = Config { seed, sim_distance, ..Config::default() };
        let world = Arc::new(Mutex::new(World::load(&dir).expect("a world in the temp folder")));
        let (tx, rx) = channel();
        let mut game = Game::new(cfg, dir.clone(), rx, Box::new(|_| {}));
        terrain::attach(&mut game, world.clone());
        Harness { game, world, dir, tx, inboxes: HashMap::new(), frames: HashMap::new() }
    }

    /// A survival session `id` joins and stands at `pos`.
    pub fn join(&mut self, id: u32, name: &str, pos: [f64; 3]) {
        let (to_viewer, rx) = unbounded_channel();
        self.inboxes.insert(id, rx);
        self.send(GameMsg::Join { id, name: name.into(), to_viewer, mode: proto::GM_SURVIVAL, survival_client: true });
        self.move_to(id, pos);
    }

    /// The session's next state puts its player at `pos`.
    pub fn move_to(&mut self, id: u32, pos: [f64; 3]) {
        let state = PlayerState { pos, yaw: 0.0, pitch: 0.0, flags: 0, swings: 0, held: 0 };
        self.send(GameMsg::State { id, state });
    }

    pub fn leave(&mut self, id: u32) {
        self.send(GameMsg::Leave { id });
    }

    /// A survival frame from the session, as the hub passes it on.
    pub fn push_frame(&mut self, id: u32, tag: u8, body: &[u8]) {
        self.send(GameMsg::Frame { id, tag, body: body.to_vec() });
    }

    pub fn send(&mut self, m: GameMsg) {
        self.tx.send(m).unwrap();
    }

    /// `n` ticks back to back (the messages sent so far go in with the first).
    pub fn tick(&mut self, n: u32) {
        for _ in 0..n {
            self.game.tick();
        }
        self.collect();
    }

    /// Ticks, giving the generation workers time, until every player's chunks are final.
    pub fn settle(&mut self) {
        let start = Instant::now();
        self.tick(1);
        while terrain::pending(&self.game) > 0 {
            assert!(start.elapsed() < Duration::from_secs(300), "terrain did not settle: {} chunks pending", terrain::pending(&self.game));
            std::thread::sleep(Duration::from_millis(2));
            self.tick(1);
        }
    }

    /// Every frame session `id` got since the last call, as `(tag, body)`.
    pub fn frames_for(&mut self, id: u32) -> Vec<(u8, Vec<u8>)> {
        self.collect();
        self.frames.remove(&id).unwrap_or_default()
    }

    fn collect(&mut self) {
        for (&id, rx) in &mut self.inboxes {
            while let Ok(bytes) = rx.try_recv() {
                let frames = proto::split_frames(&bytes).expect("whole frames");
                self.frames.entry(id).or_default().extend(frames.into_iter().map(|(t, b)| (t, b.to_vec())));
            }
        }
    }
}

impl Drop for Harness {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}
