//! Falling blocks after an edit or a server change: `BlockFalling.checkFallable` (sand, gravel,
//! concrete powder, anvils), `BlockDragonEgg.checkFall`, and where the `EntityFallingBlock` they
//! spawn comes to rest (`EntityFallingBlock.onUpdate`), with `BlockConcretePowder.onEndFalling` and
//! `tryTouchWater`.
//!
//! The `PopWorld` has no pending tick list outside population, so `BlockFalling.onBlockAdded` and
//! `neighborChanged` schedule nothing there. [`settle`] stands in for those ticks: it walks the
//! world's change log and checks every changed block and the block above it.
//!
//! [`settle_with`] with a list hands the falling blocks back as [`Fall`]s for the server to spawn as
//! entities. [`settle`] (the viewer's own world, no entity layer) still lands the block in the same
//! call.
//!
//! DROPPED: the `tickRate` delay of 2 ticks of `BlockFalling.updateTick`; the entity spawns at once.

use worldgen::blocks::{self, block_of, id};
use worldgen::populate::behavior::can_fall_through;
use worldgen::populate::props::material;
use worldgen::populate::state::{mat, Facing};
use worldgen::populate::{PopWorld, Pos};

/// `Block.registerBlocks` ids the worldgen table does not name.
const DRAGON_EGG: u16 = 122;
const ANVIL: u16 = 145;
const CONCRETE: u16 = 251;
const CONCRETE_POWDER: u16 = 252;

/// The block falls when nothing holds it: a `BlockFalling` subclass or the dragon egg.
pub fn falls(s: u16) -> bool {
    matches!(block_of(s), id::SAND | id::GRAVEL | CONCRETE_POWDER | ANVIL | DRAGON_EGG)
}

/// Checks every block the change log holds from index `from` on, and the block above each, and
/// lets what falls fall. Blocks that fall write to the log too, so a column of sand comes down
/// whole. Does nothing when the world keeps no log.
pub fn settle(w: &mut PopWorld, from: usize) {
    settle_with(w, from, None);
}

/// A block that started to fall: `EntityFallingBlock`'s spawn, at the cell the block stood in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Fall {
    pub at: (i32, i32, i32),
    pub state: u16,
}

/// [`settle`], with the falling blocks handed back as [`Fall`]s instead of landing at once: the
/// source block is taken away (the entity's first tick) and the owner spawns the entity and
/// places the block where it comes to rest (`sim::falling_block`). With `None` the block lands in
/// the same call.
pub fn settle_with(w: &mut PopWorld, from: usize, mut falls: Option<&mut Vec<Fall>>) {
    let mut i = from;
    loop {
        let Some(&(p, _)) = w.changes.as_ref().and_then(|log| log.get(i)) else { return };
        i += 1;
        check(w, p, &mut falls);
        check(w, p.up(), &mut falls);
    }
}

/// `BlockFalling.checkFallable` (and `BlockDragonEgg.checkFall`, the same test) with the entity
/// branch, as `updateTick` runs it.
fn check(w: &mut PopWorld, p: Pos, out: &mut Option<&mut Vec<Fall>>) {
    // A read never generates a chunk here (`World.isAreaLoaded` in `updateTick`'s caller).
    if !w.is_chunk_loaded(p.x >> 4, p.z >> 4) {
        return;
    }
    let s = w.get(p);
    if !falls(s) {
        return;
    }
    // BlockConcretePowder.onBlockAdded / neighborChanged: tryTouchWater first.
    if block_of(s) == CONCRETE_POWDER && touches_water(w, p) {
        w.set3(p, blocks::state(CONCRETE, s & 15));
        return;
    }
    let down = p.down();
    if !((w.is_air(down) || can_fall_through(w.get(down))) && p.y >= 0) {
        return;
    }
    // EntityFallingBlock.onUpdate, first tick: the block it came from becomes air.
    w.set_air(p);
    if let Some(out) = out {
        out.push(Fall { at: (p.x, p.y, p.z), state: s });
        return;
    }
    // The entity falls through every block canFallThrough lets it; concrete powder stops in the
    // first water it enters (flag1).
    let powder = block_of(s) == CONCRETE_POWDER;
    let mut at = down;
    loop {
        let here = w.get(at);
        if powder && mat(here) == material::WATER {
            break;
        }
        let below = w.get(at.down());
        if !(mat(below) == material::AIR || can_fall_through(below)) || at.y <= 0 {
            break;
        }
        at = at.down();
    }
    if at.y < 1 {
        // DROPPED: the item drop when it falls out of the world (fallTime > 100 below y 1).
        return;
    }
    // DROPPED: the item drop when `World.mayPlace` refuses. The entity's box passes through blocks
    // with no collision box (a torch, a flower) and lands on the block below them; this lands on
    // top of any block canFallThrough refuses.
    let hit = w.get(at);
    w.set3(at, s);
    // BlockConcretePowder.onEndFalling: landing in a liquid sets it.
    if powder && matches!(mat(hit), material::WATER | material::LAVA) {
        w.set3(at, blocks::state(CONCRETE, s & 15));
    }
}

/// `BlockConcretePowder.tryTouchWater`: water on any side but below.
fn touches_water(w: &mut PopWorld, p: Pos) -> bool {
    [Facing::Up, Facing::North, Facing::South, Facing::West, Facing::East].iter().any(|&f| {
        let q = p.offset(f);
        w.is_chunk_loaded(q.x >> 4, q.z >> 4) && mat(w.get(q)) == material::WATER
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use worldgen::populate::PopStatics;
    use worldgen::ChunkGenerator;

    /// A loaded chunk at (0, 0) with a clear column of air from y 100 up, over stone at y 99.
    fn world(gen: &ChunkGenerator) -> PopWorld<'_> {
        let mut w = PopWorld::new(gen, PopStatics::default());
        for y in 99..120 {
            for x in 0..4 {
                w.set(Pos::new(x, y, 0), if y == 99 { blocks::state(1, 0) } else { blocks::AIR }, 2);
            }
        }
        w.changes = Some(Vec::new());
        w
    }

    fn place(w: &mut PopWorld, p: Pos, s: u16) {
        let from = w.changes.as_ref().unwrap().len();
        w.set(p, s, 3);
        settle(w, from);
    }

    #[test]
    fn sand_and_gravel_fall_to_the_ground_and_columns_follow() {
        let gen = ChunkGenerator::new(1);
        let mut w = world(&gen);
        place(&mut w, Pos::new(0, 110, 0), blocks::SAND);
        assert_eq!(w.get(Pos::new(0, 110, 0)), blocks::AIR);
        assert_eq!(w.get(Pos::new(0, 100, 0)), blocks::SAND);
        // Red sand keeps its variant (the entity carries the whole state).
        let red = blocks::state(id::SAND, 1);
        place(&mut w, Pos::new(0, 115, 0), red);
        assert_eq!(w.get(Pos::new(0, 101, 0)), red);
        // Stacked on a support that goes away, the whole column comes down.
        place(&mut w, Pos::new(1, 105, 0), blocks::state(1, 0));
        w.set(Pos::new(1, 106, 0), blocks::GRAVEL, 2);
        w.set(Pos::new(1, 107, 0), blocks::SAND, 2);
        place(&mut w, Pos::new(1, 105, 0), blocks::AIR);
        assert_eq!(w.get(Pos::new(1, 100, 0)), blocks::GRAVEL);
        assert_eq!(w.get(Pos::new(1, 101, 0)), blocks::SAND);
        assert_eq!(w.get(Pos::new(1, 107, 0)), blocks::AIR);
        // A block that holds stays put.
        place(&mut w, Pos::new(2, 100, 0), blocks::SAND);
        assert_eq!(w.get(Pos::new(2, 100, 0)), blocks::SAND);
    }

    #[test]
    fn with_a_fall_list_the_block_becomes_an_entity_spawn() {
        let gen = ChunkGenerator::new(1);
        let mut w = world(&gen);
        let mut falls = Vec::new();
        let from = w.changes.as_ref().unwrap().len();
        w.set(Pos::new(0, 110, 0), blocks::SAND, 3);
        settle_with(&mut w, from, Some(&mut falls));
        assert_eq!(falls, vec![Fall { at: (0, 110, 0), state: blocks::SAND }]);
        assert_eq!(w.get(Pos::new(0, 110, 0)), blocks::AIR);
        assert_eq!(w.get(Pos::new(0, 100, 0)), blocks::AIR);
    }

    #[test]
    fn a_player_edit_and_a_server_change_both_let_sand_fall() {
        use crate::collision::BlockAccess;
        use crate::terrain::{Edit, Pos as CPos, Terrain};
        // `populate` recurses deeply through neighbour updates.
        let t = std::thread::Builder::new()
            .stack_size(256 << 20)
            .spawn(|| {
                let gen = ChunkGenerator::new(1);
                let mut t = Terrain::new(&gen);
                let wanted = |c: CPos| c == (0, 0);
                for q in t.needed((0, 0)) {
                    t.insert_generated(q, gen.generate(q.0, q.1), wanted);
                }
                assert!(t.ensure((0, 0), wanted));
                let high = (8, t.world.get_height(8, 8) + 20, 8);
                let changes = t.edit(&[Edit { at: high, s: blocks::SAND, flags: 3, place: true }]);
                assert!(changes.iter().any(|&((x, y, z), s)| (x, z) == (8, 8) && y < high.1 && s == blocks::SAND), "{changes:?}");
                // The placed block's final state goes back last: air, it fell.
                assert_eq!(changes.last(), Some(&(high, blocks::AIR)));
                let high = (9, t.world.get_height(9, 8) + 20, 8);
                let changes = t.set(high, blocks::GRAVEL, 3);
                assert!(changes.iter().any(|&((x, y, z), s)| (x, z) == (9, 8) && y < high.1 && s == blocks::GRAVEL), "{changes:?}");
                assert_eq!(t.state(high.0, high.1, high.2), blocks::AIR);
            })
            .unwrap();
        t.join().unwrap();
    }

    #[test]
    fn concrete_powder_sets_in_water() {
        let gen = ChunkGenerator::new(1);
        let mut w = world(&gen);
        let water = blocks::state(id::WATER, 0);
        w.set(Pos::new(3, 100, 0), water, 2);
        w.set(Pos::new(3, 101, 0), water, 2);
        place(&mut w, Pos::new(3, 110, 0), blocks::state(CONCRETE_POWDER, 5));
        // It stops in the first water it enters and becomes concrete of the same color.
        assert_eq!(w.get(Pos::new(3, 101, 0)), blocks::state(CONCRETE, 5));
        assert_eq!(w.get(Pos::new(3, 110, 0)), blocks::AIR);
    }
}
