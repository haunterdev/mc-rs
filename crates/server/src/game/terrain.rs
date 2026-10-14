//! The server's terrain: a `sim::terrain::Terrain` over a `worldgen` `ChunkGenerator` for the
//! world seed. Two worker threads generate; population runs on the game thread, the way the
//! viewer's populator thread runs it. Every chunk within `sim_distance` chunks of a player is kept
//! final, held chunks past `sim_distance + 3` are packed, and the overlay starts from the saved
//! world's edits (`World::all_edits`).
//!
//! Population runs strictly in [`plan`] order and only through `Terrain::ensure` (a generated
//! chunk comes in with nothing wanted), so what the game makes does not depend on which worker
//! answered first: the same seed and the same players give the same chunks as `Terrain` run
//! alone in that order.
//!
//! Light comes from `sim::region` per chunk on first ask and is dropped when an edit lands within
//! 15 blocks. It is computed on the game thread.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crossbeam_channel::{unbounded, Receiver, Sender};
use proto::BlockChange;
use serde_json::Value;
use sim::collision::BlockAccess;
use sim::region::{ColumnLight, Region};
use sim::terrain::{dist2, Pos, Terrain};
use worldgen::{Chunk, ChunkGenerator};

use super::{net, Game};
use crate::world::World;

/// `populate` recurses deeply through neighbour updates (falling sand, liquids); the game thread
/// and the tests that drive a game run on a stack this size.
pub const STACK: usize = 256 << 20;
/// Generation threads.
const WORKERS: usize = 2;
/// Population time per tick, so a player arriving does not stall the tick for seconds.
const BUDGET: Duration = Duration::from_millis(20);
/// Held chunks this many chunks past `sim_distance` are packed.
const PACK_MARGIN: i32 = 3;
/// Ticks between two packing passes while the players stand still.
const PACK_EVERY: u64 = 20;
/// An edit drops the cached light of every chunk within this many blocks of it.
const LIGHT_REACH: i32 = 15;

#[derive(Default)]
pub struct State {
    core: Option<Core>,
}

struct Core {
    terrain: Terrain<'static>,
    world: Arc<Mutex<World>>,
    /// Chunks for the workers to generate; dropping it ends them.
    jobs: Sender<Pos>,
    generated: Receiver<(Pos, Chunk)>,
    /// Asked of the workers and not back yet.
    requested: HashSet<Pos>,
    /// The players' chunks, sorted.
    centers: Vec<Pos>,
    /// Chunks in range that are not final and held yet, in [`plan`] order.
    pending: Vec<Pos>,
    light: HashMap<Pos, ColumnLight>,
}

/// Every chunk within `sim` chunks of a center (`dist2`, the circle `pack_beyond` uses), nearest
/// to any center first, then by x and z.
pub fn plan(centers: &[Pos], sim: i32) -> Vec<Pos> {
    let mut out: Vec<(i32, Pos)> = Vec::new();
    let mut seen = HashSet::new();
    for &c in centers {
        for x in c.0 - sim..=c.0 + sim {
            for z in c.1 - sim..=c.1 + sim {
                let p = (x, z);
                if dist2(p, c) <= sim * sim && seen.insert(p) {
                    let d = centers.iter().map(|&o| dist2(p, o)).min().unwrap_or(0);
                    out.push((d, p));
                }
            }
        }
    }
    out.sort_unstable();
    out.into_iter().map(|(_, p)| p).collect()
}

/// Gives the game its terrain over the hub's world. A game without it (the hook tests) has no
/// terrain: `state` is air and `set_block` does nothing.
pub fn attach(game: &mut Game, world: Arc<Mutex<World>>) {
    // One generator for the life of the game: the `Terrain` borrows it for `'static`, and the
    // workers share it.
    let gen: &'static ChunkGenerator = Box::leak(Box::new(ChunkGenerator::new(game.cfg.seed)));
    let (jobs, job_rx) = unbounded::<Pos>();
    let (done, generated) = unbounded();
    for i in 0..WORKERS {
        let (rx, tx): (Receiver<Pos>, Sender<(Pos, Chunk)>) = (job_rx.clone(), done.clone());
        std::thread::Builder::new()
            .name(format!("terrain-gen-{i}"))
            .spawn(move || {
                for p in rx {
                    if tx.send((p, gen.generate(p.0, p.1))).is_err() {
                        return;
                    }
                }
            })
            .expect("spawn a terrain worker");
    }
    let mut terrain = Terrain::new(gen);
    let edits = world.lock().unwrap().all_edits();
    terrain.remote(&edits);
    game.terrain.core = Some(Core {
        terrain,
        world,
        jobs,
        generated,
        requested: HashSet::new(),
        centers: Vec::new(),
        pending: Vec::new(),
        light: HashMap::new(),
    });
}

pub fn on_join(_game: &mut Game, _id: u32) {}

pub fn on_leave(_game: &mut Game, _id: u32) {}

pub fn on_frame(_game: &mut Game, _id: u32, _tag: u8, _body: &[u8]) -> bool {
    false
}

/// The players' chunks, sorted, from the last state each sent.
fn centers(game: &Game) -> Vec<Pos> {
    let mut c: Vec<Pos> = game
        .sessions
        .values()
        .filter_map(|s| s.state.map(|st| ((st.pos[0].floor() as i32) >> 4, (st.pos[2].floor() as i32) >> 4)))
        .collect();
    c.sort_unstable();
    c.dedup();
    c
}

pub fn tick(game: &mut Game) {
    let sim = game.cfg.sim_distance.max(0);
    let centers = centers(game);
    let ticks = game.ticks;
    let Some(core) = game.terrain.core.as_mut() else { return };
    let moved = centers != core.centers;
    if moved {
        core.centers = centers;
        core.pending = plan(&core.centers, sim);
    }
    while let Ok((p, chunk)) = core.generated.try_recv() {
        core.requested.remove(&p);
        core.terrain.insert_generated(p, chunk, |_| false);
    }
    core.populate(Instant::now() + BUDGET);
    if moved || ticks % PACK_EVERY == 0 {
        core.terrain.pack_beyond(&core.centers, sim + PACK_MARGIN);
    }
    game.stats.loaded_chunks = core.terrain.world.loaded().count();
}

impl Core {
    /// Works down `pending` in order until the budget runs out or a chunk waits on generation.
    fn populate(&mut self, deadline: Instant) {
        let mut done = 0;
        for &p in &self.pending {
            if self.terrain.is_final(p) {
                self.terrain.unpack(p);
                done += 1;
                continue;
            }
            let needed = self.terrain.needed(p);
            if !needed.is_empty() {
                break;
            }
            if Instant::now() >= deadline {
                break;
            }
            // Every 2x2 `ensure` tries holds `p`, which is in range, so all four populate.
            self.terrain.ensure(p, |_| true);
            done += 1;
        }
        self.pending.drain(..done);
        // Ask for what the next chunks need, nearest first.
        for &p in &self.pending {
            for q in self.terrain.needed(p) {
                if self.requested.insert(q) && self.jobs.send(q).is_err() {
                    return;
                }
            }
        }
    }

    /// Drops the light of every chunk within [`LIGHT_REACH`] blocks of a change.
    fn drop_light(&mut self, changes: &[BlockChange]) {
        if self.light.is_empty() {
            return;
        }
        for &((x, _, z), _) in changes {
            for cx in (x - LIGHT_REACH) >> 4..=(x + LIGHT_REACH) >> 4 {
                for cz in (z - LIGHT_REACH) >> 4..=(z + LIGHT_REACH) >> 4 {
                    self.light.remove(&(cx, cz));
                }
            }
        }
    }
}

pub fn save(_game: &Game) -> Value {
    // The edits are the world's (edits.bin); generation and population come back from the seed.
    Value::Null
}

pub fn load(_game: &mut Game, _v: Value) {}

/// A session's `V_BLOCKS` batch, as the hub relayed it: into the overlay raw. The hub already
/// wrote it to edits.bin and sent it to the other viewers.
pub fn on_blocks(game: &mut Game, _id: u32, changes: &[BlockChange]) {
    let Some(core) = game.terrain.core.as_mut() else { return };
    core.terrain.remote(changes);
    core.drop_light(changes);
}

/// `World.setBlockState(at, state)` by the server (flags 3, neighbour updates): every block that
/// changed goes to edits.bin and to every session as `S_BLOCKS`, and paintings on a block that
/// became air come down. Returns the changes, in order; a chunk never generated is left alone.
pub fn set_block(game: &mut Game, at: (i32, i32, i32), state: u16) -> Vec<BlockChange> {
    let Some(core) = game.terrain.core.as_mut() else { return Vec::new() };
    let changes = core.terrain.set(at, state, 3);
    if changes.is_empty() {
        return changes;
    }
    core.drop_light(&changes);
    let gone = core.world.lock().unwrap().apply(&changes);
    net::broadcast(game, None, proto::S_BLOCKS, &proto::encode_blocks(&changes));
    for p in gone {
        net::broadcast(game, None, proto::S_PAINTING, &p.encode());
    }
    super::tiles::on_changes(game, None, &changes);
    changes
}

/// The block state at `at`: air unless its chunk is final and held, the way a client reads an
/// unloaded chunk.
pub fn state(game: &Game, at: (i32, i32, i32)) -> u16 {
    match &game.terrain.core {
        Some(core) => core.terrain.state(at.0, at.1, at.2),
        None => worldgen::blocks::AIR,
    }
}

/// The chunk is final and held.
pub fn is_final(game: &Game, p: Pos) -> bool {
    game.terrain.core.as_ref().is_some_and(|c| c.terrain.is_final(p) && c.terrain.world.is_chunk_loaded(p.0, p.1))
}

/// Chunks in range not final yet; 0 once every player's surroundings are ready.
pub fn pending(game: &Game) -> usize {
    game.terrain.core.as_ref().map_or(0, |c| c.pending.len())
}

/// `(sky, block)` light at `pos`, from the chunk's light computed with its eight neighbours. None
/// until the 3x3 around the chunk is final, and outside 0..256.
pub fn light(game: &mut Game, pos: (i32, i32, i32)) -> Option<(u8, u8)> {
    let (x, y, z) = pos;
    if !(0..256).contains(&y) {
        return None;
    }
    let core = game.terrain.core.as_mut()?;
    let c = (x >> 4, z >> 4);
    if !core.light.contains_key(&c) {
        let mut chunks = Vec::with_capacity(9);
        for dz in -1..=1 {
            for dx in -1..=1 {
                chunks.push(core.terrain.final_chunk((c.0 + dx, c.1 + dz))?);
            }
        }
        let refs: [&Chunk; 9] = std::array::from_fn(|i| &chunks[i]);
        core.light.insert(c, Region::new(refs).center_light());
    }
    Some(core.light[&c].get(x & 15, y, z & 15))
}

#[cfg(test)]
#[path = "terrain_tests.rs"]
mod tests;
