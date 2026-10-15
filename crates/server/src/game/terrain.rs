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
    // The saved `/difficulty` outranks server.json; either way the world holds the live id.
    {
        let mut w = world.lock().unwrap();
        match w.difficulty {
            Some(d) => game.cfg.difficulty = ["peaceful", "easy", "normal", "hard"][d.min(3) as usize].into(),
            None => w.difficulty = Some(super::players::difficulty(game) as u8),
        }
    }
    let mut terrain = Terrain::new(gen);
    // Falling blocks become entities (`falling_blocks`), not instant moves.
    terrain.falls = Some(Vec::new());
    let edits = world.lock().unwrap().all_edits();
    terrain.remote(&edits);
    // Flowing liquids tick from here on (sim fluids.rs); the saved edits never wake them.
    terrain.set_ticking(true);
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
pub(super) fn centers(game: &Game) -> Vec<Pos> {
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
    // WorldServer.tickUpdates: the liquids due this tick.
    let flowed = core.terrain.tick_blocks();
    if moved || ticks % PACK_EVERY == 0 {
        core.terrain.pack_beyond(&core.centers, sim + PACK_MARGIN);
    }
    game.stats.loaded_chunks = core.terrain.world.loaded().count();
    publish(game, flowed);
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
    // The viewer has no tick list, so the liquids its edits touch are woken here.
    let woke = core.terrain.wake_liquids(changes);
    publish(game, woke);
}

/// `World.setBlockState(at, state)` by the server (flags 3, neighbour updates): every block that
/// changed goes to edits.bin and to every session as `S_BLOCKS`, and paintings on a block that
/// became air come down. Returns the changes, in order; a chunk never generated is left alone.
pub fn set_block(game: &mut Game, at: (i32, i32, i32), state: u16) -> Vec<BlockChange> {
    let Some(core) = game.terrain.core.as_mut() else { return Vec::new() };
    let changes = core.terrain.set(at, state, 3);
    publish(game, changes.clone());
    changes
}

/// The blocks that started to fall since the last call (`BlockFalling.checkFallable`).
pub fn take_falls(game: &mut Game) -> Vec<sim::gravity::Fall> {
    game.terrain.core.as_mut().map(|c| c.terrain.take_falls()).unwrap_or_default()
}

/// [`set_block`] that also returns the state each change replaced (same order), for the drops of
/// what a neighbour update broke.
pub fn set_block_logged(game: &mut Game, at: (i32, i32, i32), state: u16) -> (Vec<BlockChange>, Vec<u16>) {
    set_flags(game, at, state, 3)
}

/// [`set_block_logged`] with `World.setBlockState`'s flags (1 notifies the neighbours). Every
/// change is published whatever the flags: the viewers and edits.bin always get it.
pub fn set_flags(game: &mut Game, at: (i32, i32, i32), state: u16, flags: i32) -> (Vec<BlockChange>, Vec<u16>) {
    let Some(core) = game.terrain.core.as_mut() else { return (Vec::new(), Vec::new()) };
    let (changes, olds) = core.terrain.set_logged(at, state, flags);
    publish(game, changes.clone());
    (changes, olds)
}

/// `f` on the population world around `at` (a tree growing), the blocks it wrote published like
/// [`set_block`]'s. None when the chunk was never generated.
pub fn edit_world<R>(game: &mut Game, at: (i32, i32, i32), f: impl FnOnce(&mut worldgen::populate::PopWorld<'static>) -> R) -> Option<(R, Vec<BlockChange>)> {
    let core = game.terrain.core.as_mut()?;
    let (out, changes) = core.terrain.edit_world(at, f)?;
    publish(game, changes.clone());
    Some((out, changes))
}

/// `f` on the population world for reads that need its helpers (`isRainingAt`). Nothing written
/// here is published, and `f` must only touch held chunks.
pub fn peek<R>(game: &mut Game, f: impl FnOnce(&mut worldgen::populate::PopWorld<'static>) -> R) -> Option<R> {
    let core = game.terrain.core.as_mut()?;
    Some(f(&mut core.terrain.world))
}

/// Whether section `sy` of the held chunk has storage (`ExtendedBlockStorage != NULL_BLOCK_STORAGE`).
pub fn has_section(game: &Game, cx: i32, cz: i32, sy: usize) -> bool {
    game.terrain.core.as_ref().and_then(|c| c.terrain.world.chunk_ref(cx, cz)).is_some_and(|c| c.sections[sy].is_some())
}

/// Blocks the server's own world wrote: to edits.bin and to every session as `S_BLOCKS`, and
/// paintings on a block that became air come down.
fn publish(game: &mut Game, changes: Vec<BlockChange>) {
    let Some(core) = game.terrain.core.as_mut() else { return };
    if changes.is_empty() {
        return;
    }
    core.drop_light(&changes);
    let gone = core.world.lock().unwrap().apply(&changes);
    net::broadcast(game, None, proto::S_BLOCKS, &proto::encode_blocks(&changes));
    for p in gone {
        net::broadcast(game, None, proto::S_PAINTING, &p.encode());
    }
    super::tiles::on_changes(game, None, &changes);
}

/// The block state at `at`: air unless its chunk is final and held, the way a client reads an
/// unloaded chunk.
pub fn state(game: &Game, at: (i32, i32, i32)) -> u16 {
    match &game.terrain.core {
        Some(core) => core.terrain.state(at.0, at.1, at.2),
        None => worldgen::blocks::AIR,
    }
}

/// The game's terrain as the collision and liquid code reads it (`World.getBlockState` for an
/// entity on the server).
pub struct Blocks<'a>(pub &'a Game);

impl sim::collision::BlockAccess for Blocks<'_> {
    fn state(&self, x: i32, y: i32, z: i32) -> u16 {
        state(self.0, (x, y, z))
    }

    fn loaded(&self, x: i32, z: i32) -> bool {
        self.0.terrain.core.as_ref().is_some_and(|c| sim::collision::BlockAccess::loaded(&c.terrain, x, z))
    }
}

/// The world file the terrain was attached to (game rules, spawn points).
pub fn world(game: &Game) -> Option<Arc<Mutex<World>>> {
    game.terrain.core.as_ref().map(|c| c.world.clone())
}

/// A boolean game rule (`GameRules.getBoolean`), `default` while no world is attached.
pub fn rule(game: &Game, name: &str, default: bool) -> bool {
    match world(game) {
        Some(w) => w.lock().unwrap().rule(name).map_or(default, |v| v == "true"),
        None => default,
    }
}

/// `Chunk.getHeightValue` and `Chunk.getTopFilledSegment` of the column's chunk, if it is held.
pub fn column(game: &Game, x: i32, z: i32) -> Option<(i32, i32)> {
    let c = game.terrain.core.as_ref()?.terrain.world.chunk_ref(x >> 4, z >> 4)?;
    Some((c.height_value(x & 15, z & 15), c.top_filled_segment()))
}

/// `World.getBiome(pos)` for a held chunk.
pub fn biome(game: &Game, x: i32, z: i32) -> Option<&'static worldgen::biome::Biome> {
    let w = &game.terrain.core.as_ref()?.terrain.world;
    w.is_chunk_loaded(x >> 4, z >> 4).then(|| w.biome(worldgen::populate::Pos::new(x, 0, z)))
}

/// `World.getTopSolidOrLiquidBlock(pos)`'s y, for a held chunk.
pub fn top_solid_or_liquid(game: &mut Game, x: i32, z: i32) -> Option<i32> {
    let w = &mut game.terrain.core.as_mut()?.terrain.world;
    w.is_chunk_loaded(x >> 4, z >> 4).then(|| w.top_solid_or_liquid(worldgen::populate::Pos::new(x, 0, z)).y)
}

/// The chunk has been populated (its `populate` ran, whether or not its neighbours have).
pub fn is_populated(game: &Game, p: Pos) -> bool {
    game.terrain.core.as_ref().is_some_and(|c| c.terrain.is_populated(p) && c.terrain.world.is_chunk_loaded(p.0, p.1))
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
