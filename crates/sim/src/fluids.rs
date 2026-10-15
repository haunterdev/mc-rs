//! Flowing water and lava after population: the terrain's pending tick list (`WorldServer`'s
//! `pendingTickListEntriesTreeSet`, `worldgen::populate::ticks`) and `WorldServer.tickUpdates`
//! running it. `BlockDynamicLiquid` itself is `worldgen::populate::liquid`, unchanged.
//!
//! The list is off until the owner turns it on ([`Terrain::set_ticking`]): the server and a
//! single-player viewer tick, a viewer in a server session does not (the server's ticks reach it
//! as `S_BLOCKS`). Population never sees the list, so it stays bit-exact.

use worldgen::blocks;
use worldgen::populate::liquid;
use worldgen::populate::props::material;
use worldgen::populate::state::{self, Facing};
use worldgen::populate::world::TickRand;
use worldgen::populate::{behavior, Pos as WPos};

use super::{BlockChange, Terrain};

/// A tick only runs where every chunk within this many blocks is final and held: a liquid reads
/// up to five blocks out (`getSlopeDistance`) and its neighbour updates one further, and reading a
/// chunk that is not held would generate it raw.
const REACH: i32 = 8;

impl Terrain<'_> {
    /// Turns the pending tick list on or off. Off drops what is pending.
    pub fn set_ticking(&mut self, on: bool) {
        self.ticking = on;
        if !on {
            self.ticks = Default::default();
        }
    }

    /// Ticks waiting on the list.
    pub fn pending_ticks(&self) -> usize {
        self.ticks.len()
    }

    /// Lends the list to the world for an edit, a set or a tick.
    pub(super) fn lend_ticks(&mut self) {
        if self.ticking {
            self.world.ticks = Some(std::mem::take(&mut self.ticks));
        }
    }

    pub(super) fn return_ticks(&mut self) {
        if let Some(t) = self.world.ticks.take() {
            self.ticks = t;
        }
    }

    /// Every chunk within [`REACH`] blocks of `(x, z)` is final and held.
    fn tick_ready(&self, x: i32, z: i32) -> bool {
        for cx in (x - REACH) >> 4..=(x + REACH) >> 4 {
            for cz in (z - REACH) >> 4..=(z + REACH) >> 4 {
                if !(self.is_final((cx, cz)) && self.world.is_chunk_loaded(cx, cz)) {
                    return false;
                }
            }
        }
        true
    }

    /// Blocks another world settled (`remote`), seen by the world that ticks: each liquid at or
    /// beside a change gets the update its own world would have given it (`liquid::wake`).
    /// Returns every block that wrote, in order.
    pub fn wake_liquids(&mut self, changes: &[BlockChange]) -> Vec<BlockChange> {
        if !self.ticking || changes.is_empty() {
            return Vec::new();
        }
        self.world.changes = Some(Vec::new());
        self.lend_ticks();
        for &((x, y, z), _) in changes {
            if !(0..256).contains(&y) || !self.tick_ready(x, z) {
                continue;
            }
            let p = WPos::new(x, y, z);
            let s = self.world.get(p);
            liquid::wake(&mut self.world, p, s, false);
            for f in Facing::VALUES {
                let q = p.offset(f);
                if (0..256).contains(&q.y) {
                    let s = self.world.get(q);
                    liquid::wake(&mut self.world, q, s, true);
                }
            }
        }
        self.return_ticks();
        let changes = self.take_changes();
        self.remember(&changes);
        changes
    }

    /// One tick of `WorldServer.tickUpdates(false)`: the world time moves on and every entry due
    /// runs `Block.updateTick` with `World.rand`. Returns every block written, in order.
    pub fn tick_blocks(&mut self) -> Vec<BlockChange> {
        if !self.ticking {
            return Vec::new();
        }
        // WorldServer.tick: setWorldTotalTime(+1), then tickUpdates.
        self.ticks.now += 1;
        if self.ticks.is_empty() {
            return Vec::new();
        }
        let due = self.ticks.take_due();
        if due.is_empty() {
            return Vec::new();
        }
        self.world.changes = Some(Vec::new());
        self.lend_ticks();
        for (at, block) in due {
            if self.tick_ready(at.0, at.2) {
                let p = WPos::new(at.0, at.1, at.2);
                let s = self.world.get(p);
                if state::mat(s) != material::AIR && blocks::block_of(s) == block {
                    behavior::update_tick(&mut self.world, p, s, TickRand::World);
                }
            } else if let Some(t) = self.world.ticks.as_mut() {
                // The area is not loaded: `scheduleUpdate(pos, block, 0)`, as vanilla.
                t.schedule(at, block, 0, 0);
            }
        }
        self.return_ticks();
        let changes = self.take_changes();
        self.remember(&changes);
        changes
    }
}

#[cfg(test)]
mod tests {
    use super::super::Edit;
    use crate::collision::BlockAccess;
    use super::*;
    use worldgen::blocks::{id, state as st};
    use worldgen::ChunkGenerator;

    /// Runs `f` on a big stack (population recurses deeply).
    fn run(f: impl FnOnce() + Send + 'static) {
        std::thread::Builder::new().stack_size(256 << 20).spawn(f).unwrap().join().unwrap();
    }

    /// A terrain with the 5x5 around (0, 0) final, ticking.
    fn terrain(gen: &ChunkGenerator) -> Terrain<'_> {
        let mut t = Terrain::new(gen);
        for x in -3..=3 {
            for z in -3..=3 {
                for q in t.needed((x, z)) {
                    if !t.known(q) {
                        t.insert_generated(q, gen.generate(q.0, q.1), |_| false);
                    }
                }
                t.ensure((x, z), |_| true);
            }
        }
        t.set_ticking(true);
        t
    }

    fn put(t: &mut Terrain, at: (i32, i32, i32), s: u16) {
        t.edit(&[Edit { at, s, flags: 3, place: false }]);
    }

    fn run_ticks(t: &mut Terrain, n: u32) -> Vec<BlockChange> {
        (0..n).flat_map(|_| t.tick_blocks()).collect()
    }

    /// A stone floor at y 200 with a source on it spreads 7 blocks out and down off the edge,
    /// settles, and drains once the source is gone.
    #[test]
    fn water_spreads_settles_and_drains() {
        run(|| {
            let gen = ChunkGenerator::new(5);
            let mut t = terrain(&gen);
            for x in -9..=9 {
                for z in -9..=9 {
                    put(&mut t, (x, 200, z), st(id::STONE, 0));
                }
            }
            put(&mut t, (0, 201, 0), st(id::FLOWING_WATER, 0));
            assert!(t.pending_ticks() > 0, "a placed flowing source schedules itself");
            run_ticks(&mut t, 400);
            assert_eq!(t.pending_ticks(), 0, "the flow settles");
            let at = |t: &Terrain, x, y, z| t.state(x, y, z);
            assert_eq!(at(&t, 0, 201, 0), st(id::WATER, 0), "the source turns still");
            assert_eq!(blocks::block_of(at(&t, 3, 201, 0)), id::WATER);
            assert_eq!(at(&t, 3, 201, 0) & 15, 3, "level 3, three blocks out");
            assert_eq!(at(&t, 0, 201, 7) & 15, 7, "the last level");
            assert_eq!(at(&t, 0, 201, 8), blocks::AIR, "no further than 7");
            assert_eq!(at(&t, 5, 201, 5), blocks::AIR, "manhattan distance 10 stays dry");
            // Take the source away: the flow drains.
            put(&mut t, (0, 201, 0), blocks::AIR);
            run_ticks(&mut t, 400);
            for x in -8..=8 {
                assert_eq!(at(&t, x, 201, 0), blocks::AIR, "drained at x {x}");
            }
        });
    }

    /// Lava runs 3 blocks on a flat floor (2 a step) and a water flow over it makes cobblestone or
    /// obsidian.
    #[test]
    fn lava_spreads_slower_and_mixes() {
        run(|| {
            let gen = ChunkGenerator::new(5);
            let mut t = terrain(&gen);
            for x in -9..=9 {
                for z in -9..=9 {
                    put(&mut t, (x, 200, z), st(id::STONE, 0));
                }
            }
            put(&mut t, (0, 201, 0), st(id::FLOWING_LAVA, 0));
            run_ticks(&mut t, 29);
            assert_eq!(t.state(1, 201, 0), blocks::AIR, "lava waits 30 ticks");
            run_ticks(&mut t, 1200);
            assert_eq!(blocks::block_of(t.state(3, 201, 0)), id::LAVA);
            assert_eq!(t.state(4, 201, 0), blocks::AIR);
            // Water beside the lava source turns it to obsidian.
            put(&mut t, (0, 201, 1), st(id::FLOWING_WATER, 0));
            run_ticks(&mut t, 50);
            assert_eq!(t.state(0, 201, 0), st(id::OBSIDIAN, 0));
        });
    }

    /// The server's path: a viewer's raw edit next to still water wakes it.
    #[test]
    fn a_remote_edit_wakes_still_water() {
        run(|| {
            let gen = ChunkGenerator::new(5);
            let mut t = terrain(&gen);
            // A pool walled in at y 201: a still source at (0, 201, 0), stone around it.
            for x in -2..=2 {
                for z in -2..=2 {
                    t.remote(&[((x, 200, z), st(id::STONE, 0)), ((x, 201, z), st(id::STONE, 0))]);
                }
            }
            t.remote(&[((0, 201, 0), st(id::WATER, 0))]);
            let woke = t.wake_liquids(&[((0, 201, 0), st(id::WATER, 0))]);
            assert!(woke.is_empty() && t.pending_ticks() == 0, "a still source in a closed pool stays");
            // Open a wall, as a viewer's break arriving raw.
            let broke = [((1, 201, 0), blocks::AIR)];
            t.remote(&broke);
            let woke = t.wake_liquids(&broke);
            assert_eq!(woke, vec![((0, 201, 0), st(id::FLOWING_WATER, 0))], "still turns flowing");
            run_ticks(&mut t, 20);
            assert_eq!(blocks::block_of(t.state(1, 201, 0)), id::WATER, "it flows into the gap");
        });
    }
}
