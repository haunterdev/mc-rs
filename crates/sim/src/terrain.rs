//! `Terrain`: the channel-free core of the viewer's populator, which the viewer's population thread
//! and the server's game thread both drive. It owns the `PopWorld`, runs
//! `ChunkGeneratorOverworld.populate` the way `Chunk.populate(provider, generator)` schedules it,
//! packs chunks away out of range and keeps the overlay of every block a player changed.
//!
//! `Chunk.populate` populates chunk `q` once `q`, `q + x`, `q + z` and `q + x + z` are loaded; the
//! pass writes into the 2x2 block area offset by 8, so chunk `r` is final once `r`, `r - x`,
//! `r - z` and `r - x - z` are populated. Chunks far from the player are packed away, the way the
//! server saves them, and come back before a population that may touch them.
//!
//! Which chunks are wanted (the viewer's ensures, the server's players) stays with the caller and
//! comes in as a predicate: a 2x2 is populated only when one of its chunks is wanted.

use std::collections::{HashMap, HashSet};

use worldgen::blocks;
use worldgen::populate::{self, behavior, PackedChunk, PopChunk, PopStatics, PopWorld, Pos as WPos};
use worldgen::populate::ticks::TickList;
use worldgen::{Chunk, ChunkGenerator};

use crate::collision::BlockAccess;
use crate::gravity::Fall;

#[path = "fluids.rs"]
mod fluids;

/// A chunk position.
pub type Pos = (i32, i32);
/// A block and the state it holds now (`proto::BlockChange`).
pub type BlockChange = ((i32, i32, i32), u16);

/// One `World.setBlockState(pos, s, flags)` on the server side, after `Block.canPlaceBlockAt` when
/// `place` is set (the half of `World.mayPlace` the client cannot answer without the server's
/// world).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Edit {
    pub at: (i32, i32, i32),
    pub s: u16,
    pub flags: i32,
    pub place: bool,
}

/// Squared distance between two chunk positions.
pub fn dist2(a: Pos, b: Pos) -> i32 {
    let (dx, dz) = (a.0 - b.0, a.1 - b.1);
    dx * dx + dz * dz
}

pub struct Terrain<'g> {
    pub world: PopWorld<'g>,
    /// Chunks packed away while out of range.
    store: HashMap<Pos, PackedChunk>,
    populated: HashSet<Pos>,
    /// Every block a player changed, by chunk: written over each chunk after every population
    /// that may touch it, so the shared world's edits outlast generating the chunk again.
    overlay: HashMap<Pos, HashMap<(i32, i32, i32), u16>>,
    /// The pending tick list (flowing liquids), lent to the world only around edits and
    /// [`Self::tick_blocks`], so population never schedules into it. `fluids.rs`.
    ticks: TickList,
    /// The list is on ([`Self::set_ticking`]).
    ticking: bool,
    /// The server's entity layer is on: falling blocks collect here as [`Fall`]s for the owner to
    /// spawn (`EntityFallingBlock`) instead of landing at once. [`Self::take_falls`].
    pub falls: Option<Vec<Fall>>,
}

impl<'g> Terrain<'g> {
    pub fn new(gen: &'g ChunkGenerator) -> Self {
        Terrain {
            world: PopWorld::new(gen, PopStatics::default()),
            store: HashMap::new(),
            populated: HashSet::new(),
            overlay: HashMap::new(),
            ticks: TickList::new(),
            ticking: false,
            falls: None,
        }
    }

    /// `BlockFalling.checkFallable` over the edit's change log: with the entity layer on
    /// ([`Self::falls`]) the blocks start falling, otherwise they land at once (`gravity`).
    fn settle_falls(&mut self) {
        crate::gravity::settle_with(&mut self.world, 0, self.falls.as_mut());
    }

    /// The blocks that started to fall since the last call.
    pub fn take_falls(&mut self) -> Vec<Fall> {
        self.falls.as_mut().map(std::mem::take).unwrap_or_default()
    }

    /// The chunk is held or packed.
    pub fn known(&self, p: Pos) -> bool {
        self.world.is_chunk_loaded(p.0, p.1) || self.store.contains_key(&p)
    }

    pub fn is_populated(&self, p: Pos) -> bool {
        self.populated.contains(&p)
    }

    pub fn is_packed(&self, p: Pos) -> bool {
        self.store.contains_key(&p)
    }

    pub fn is_final(&self, r: Pos) -> bool {
        [(0, 0), (-1, 0), (0, -1), (-1, -1)].iter().all(|d| self.populated.contains(&(r.0 + d.0, r.1 + d.1)))
    }

    /// The chunks of the 5x5 around `p` to generate before `p` can become final, x then z.
    pub fn needed(&self, p: Pos) -> Vec<Pos> {
        let mut out = Vec::new();
        for dx in -2..=2 {
            for dz in -2..=2 {
                let q = (p.0 + dx, p.1 + dz);
                if !self.known(q) {
                    out.push(q);
                }
            }
        }
        out
    }

    /// Populates the four 2x2s holding `p` that can be populated now; true once `p` is final.
    pub fn ensure(&mut self, p: Pos, wanted: impl Fn(Pos) -> bool) -> bool {
        for q in [(p.0 - 1, p.1 - 1), (p.0, p.1 - 1), (p.0 - 1, p.1), p] {
            self.try_populate(q, &wanted);
        }
        self.is_final(p)
    }

    /// Takes a generated chunk and populates what it completes; returns the chunks of the 3x3
    /// around `p` that are final now, x then z. A chunk a population already generated on access
    /// (`provideChunk`) keeps what was written into it, and nothing is returned.
    pub fn insert_generated(&mut self, p: Pos, c: Chunk, wanted: impl Fn(Pos) -> bool) -> Vec<Pos> {
        if self.known(p) {
            return Vec::new();
        }
        self.world.insert_chunk(PopChunk::from_generated(c));
        for q in [(p.0 - 1, p.1 - 1), (p.0, p.1 - 1), (p.0 - 1, p.1), p] {
            self.try_populate(q, &wanted);
        }
        let mut finals = Vec::new();
        for x in p.0 - 1..=p.0 + 1 {
            for z in p.1 - 1..=p.1 + 1 {
                if self.is_final((x, z)) {
                    finals.push((x, z));
                }
            }
        }
        finals
    }

    /// Packs every held chunk farther than `radius` chunks from all of `centers`.
    pub fn pack_beyond(&mut self, centers: &[Pos], radius: i32) {
        let live = radius * radius;
        let far: Vec<Pos> = self.world.loaded().filter(|&q| centers.iter().all(|&c| dist2(q, c) > live)).collect();
        for q in far {
            if let Some(chunk) = self.world.take_chunk(q.0, q.1) {
                self.store.insert(q, PackedChunk::pack(&chunk));
            }
        }
    }

    /// The player's edits, in order, stopping at the first one refused (an unknown chunk, or a
    /// place `canPlaceBlockAt` turns down). Returns every block written, then what the world
    /// holds at each edited block, so a refused or unchanged guess is put back.
    pub fn edit(&mut self, edits: &[Edit]) -> Vec<BlockChange> {
        self.world.changes = Some(Vec::new());
        self.lend_ticks();
        for e in edits {
            let c = (e.at.0 >> 4, e.at.2 >> 4);
            if !self.known(c) {
                break;
            }
            // Neighbour updates reach one block out (a plant losing its soil, leaves
            // checking for logs further on), so the 3x3 comes back from the store.
            self.unpack_around(c, -1, 1);
            let p = WPos::new(e.at.0, e.at.1, e.at.2);
            if e.place && !behavior::can_place_block_at(&mut self.world, p, e.s >> 4) {
                break;
            }
            self.world.set(p, e.s, e.flags);
        }
        // Falling blocks come down at once (`gravity`).
        self.settle_falls();
        // Flowing liquids go on the pending tick list (`fluids.rs`).
        self.return_ticks();
        let mut changes = self.take_changes();
        // What the server has at every edited block goes back last, so a refused or
        // unchanged guess is put back. An unknown chunk is skipped: reading it would
        // generate it.
        for e in edits {
            let c = (e.at.0 >> 4, e.at.2 >> 4);
            if self.known(c) {
                self.unpack_around(c, 0, 0);
                changes.push((e.at, self.world.get(WPos::new(e.at.0, e.at.1, e.at.2))));
            }
        }
        self.remember(&changes);
        changes
    }

    /// Blocks another world already settled: another player's edits, or the saved world's on
    /// joining. They join the overlay, which wins over generation and population from then on;
    /// returns what changed in chunks held or packed now.
    pub fn remote(&mut self, changes: &[BlockChange]) -> Vec<BlockChange> {
        self.remember(changes);
        let mut chunks: Vec<Pos> = changes.iter().map(|&(at, _)| (at.0 >> 4, at.2 >> 4)).collect();
        chunks.sort_unstable();
        chunks.dedup();
        let mut applied = Vec::new();
        for c in chunks {
            if self.known(c) {
                applied.extend(self.apply_overlay(c));
            }
        }
        applied
    }

    /// A change the server makes itself: `World.setBlockState(pos, s, flags)` with its neighbour
    /// updates (flags 3 for `setBlockState(pos, s)`). Returns every block written, in order; a
    /// chunk neither held nor packed is left alone.
    pub fn set(&mut self, at: (i32, i32, i32), s: u16, flags: i32) -> Vec<BlockChange> {
        let c = (at.0 >> 4, at.2 >> 4);
        if !self.known(c) {
            return Vec::new();
        }
        self.unpack_around(c, -1, 1);
        self.world.changes = Some(Vec::new());
        self.lend_ticks();
        self.world.set(WPos::new(at.0, at.1, at.2), s, flags);
        self.settle_falls();
        self.return_ticks();
        let changes = self.take_changes();
        self.remember(&changes);
        changes
    }

    /// [`Terrain::set`] with the state each returned change replaced, in the same order.
    pub fn set_logged(&mut self, at: (i32, i32, i32), s: u16, flags: i32) -> (Vec<BlockChange>, Vec<u16>) {
        self.world.olds = Some(Vec::new());
        let changes = self.set(at, s, flags);
        let olds = self.world.olds.take().unwrap_or_default();
        (changes, olds)
    }

    /// `f` on the world around `at`, the way [`Self::set`] runs an edit: the 3x3 chunks unpacked,
    /// every block `f` writes logged and returned (also remembered in the overlay), falling blocks
    /// settled. For a tree growing from a sapling. None when the chunk is neither held nor packed.
    pub fn edit_world<R>(&mut self, at: (i32, i32, i32), f: impl FnOnce(&mut PopWorld<'g>) -> R) -> Option<(R, Vec<BlockChange>)> {
        let c = (at.0 >> 4, at.2 >> 4);
        if !self.known(c) {
            return None;
        }
        self.unpack_around(c, -1, 1);
        self.world.changes = Some(Vec::new());
        self.lend_ticks();
        let out = f(&mut self.world);
        self.settle_falls();
        self.return_ticks();
        let changes = self.take_changes();
        self.remember(&changes);
        Some((out, changes))
    }

    /// Brings chunk `p` back from the store when it is packed (a player came back in range);
    /// true when it was.
    pub fn unpack(&mut self, p: Pos) -> bool {
        match self.store.remove(&p) {
            Some(packed) => {
                self.world.insert_chunk(packed.unpack());
                true
            }
            None => false,
        }
    }

    /// A final chunk's block states, for light and meshing.
    pub fn final_chunk(&mut self, r: Pos) -> Option<Chunk> {
        if !self.is_final(r) {
            return None;
        }
        Some(match self.store.get(&r) {
            Some(packed) => packed.unpack().to_chunk(),
            None => self.world.chunk(r.0, r.1).to_chunk(),
        })
    }

    fn take_changes(&mut self) -> Vec<BlockChange> {
        self.world.changes.take().unwrap_or_default().into_iter().map(|(q, s)| ((q.x, q.y, q.z), s)).collect()
    }

    /// Writes chunk `c`'s overlay into it when the chunk is held or packed; returns what changed.
    fn apply_overlay(&mut self, c: Pos) -> Vec<BlockChange> {
        let Some(entries) = self.overlay.get(&c) else { return Vec::new() };
        if let Some(packed) = self.store.remove(&c) {
            self.world.insert_chunk(packed.unpack());
        }
        let Some(chunk) = self.world.chunk_if_loaded(c.0, c.1) else { return Vec::new() };
        let mut changed = Vec::new();
        for (&(x, y, z), &s) in entries {
            if chunk.set_raw(x & 15, y, z & 15, s) {
                changed.push(((x, y, z), s));
            }
        }
        if !changed.is_empty() {
            chunk.generate_skylight_map();
        }
        changed
    }

    fn remember(&mut self, changes: &[BlockChange]) {
        for &(at, s) in changes {
            self.overlay.entry((at.0 >> 4, at.2 >> 4)).or_default().insert(at, s);
        }
    }

    /// Brings packed chunks within `lo..=hi` of `c` back into the world.
    fn unpack_around(&mut self, c: Pos, lo: i32, hi: i32) {
        for dx in lo..=hi {
            for dz in lo..=hi {
                if let Some(packed) = self.store.remove(&(c.0 + dx, c.1 + dz)) {
                    self.world.insert_chunk(packed.unpack());
                }
            }
        }
    }

    /// `Chunk.populate(provider, generator)` for chunk `q`, when it is wanted.
    fn try_populate(&mut self, q: Pos, wanted: &impl Fn(Pos) -> bool) {
        if self.populated.contains(&q) {
            return;
        }
        let quad = [q, (q.0 + 1, q.1), (q.0, q.1 + 1), (q.0 + 1, q.1 + 1)];
        if !quad.iter().all(|&c| self.known(c)) || !quad.iter().any(|&c| wanted(c)) {
            return;
        }
        // The pass can reach past its 2x2 (big trees, lakes at the edge); bring back what was
        // packed so it is written to rather than generated again.
        self.unpack_around(q, -2, 3);
        populate::populate(&mut self.world, q.0, q.1);
        self.populated.insert(q);
        if !self.overlay.is_empty() {
            for dx in -2..=3 {
                for dz in -2..=3 {
                    let c = (q.0 + dx, q.1 + dz);
                    if self.overlay.contains_key(&c) && self.world.is_chunk_loaded(c.0, c.1) {
                        self.apply_overlay(c);
                    }
                }
            }
        }
    }
}

/// Air unless the chunk is final and held, the way the client's `EmptyChunk` reads; a packed
/// chunk counts as unloaded (saved to disk), and a read never generates.
impl BlockAccess for Terrain<'_> {
    fn state(&self, x: i32, y: i32, z: i32) -> u16 {
        let c = (x >> 4, z >> 4);
        if !(0..256).contains(&y) || !self.is_final(c) {
            return blocks::AIR;
        }
        self.world.chunk_ref(c.0, c.1).map_or(blocks::AIR, |chunk| chunk.get(x & 15, y, z & 15))
    }

    fn loaded(&self, x: i32, z: i32) -> bool {
        let c = (x >> 4, z >> 4);
        self.is_final(c) && self.world.is_chunk_loaded(c.0, c.1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use worldgen::blocks::{id, state};

    /// `populate` recurses deeply through neighbour updates (falling sand, liquids).
    const STACK: usize = 256 << 20;

    #[test]
    fn set_runs_neighbour_updates() {
        let t = std::thread::Builder::new()
            .stack_size(STACK)
            .spawn(|| {
                let gen = ChunkGenerator::new(1);
                let mut t = Terrain::new(&gen);
                let wanted = |c: Pos| c == (0, 0);
                for q in t.needed((0, 0)) {
                    t.insert_generated(q, gen.generate(q.0, q.1), wanted);
                }
                assert!(t.ensure((0, 0), wanted));
                // Bedrock in the final chunk; air in a held chunk that is not final yet.
                assert!(t.world.is_chunk_loaded(2, 2) && !t.is_final((2, 2)));
                assert_eq!(t.state(8, 0, 8), state(id::BEDROCK, 0));
                assert_eq!(t.state(40, 0, 40), blocks::AIR);
                assert!(t.loaded(8, 8) && !t.loaded(40, 40));
                let top = t.world.get_height(8, 8) + 4;
                let grass = state(id::GRASS, 0);
                let flower = state(id::YELLOW_FLOWER, 0);
                assert_eq!(t.set((8, top, 8), grass, 3), vec![((8, top, 8), grass)]);
                t.set((8, top + 1, 8), flower, 3);
                assert_eq!(t.state(8, top + 1, 8), flower);
                // Taking the soil away drops the plant through `neighborChanged`.
                let broken = t.set((8, top, 8), blocks::AIR, 3);
                assert!(broken.contains(&((8, top + 1, 8), blocks::AIR)), "{broken:?}");
                assert_eq!(t.state(8, top + 1, 8), blocks::AIR);
                // An unknown chunk is left alone.
                assert!(t.set((1000, 70, 1000), grass, 3).is_empty());
            })
            .unwrap();
        t.join().unwrap();
    }
}
