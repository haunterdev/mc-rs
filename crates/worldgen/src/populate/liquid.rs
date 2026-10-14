//! `BlockLiquid`, `BlockDynamicLiquid` and `BlockStaticLiquid`: what water and lava do when they
//! are placed, when a neighbour changes, and on the immediate ticks `WorldGenLiquids` starts.

use super::behavior;
use super::props::material;
use super::state::{self, Facing, Pos};
use super::world::{PopWorld, TickRand};
use crate::blocks::{self, id};

/// `BlockLiquid.LEVEL` is the whole metadata.
#[inline]
fn level(s: u16) -> i32 {
    (s & 15) as i32
}

fn is_lava_block(b: u16) -> bool {
    b == id::FLOWING_LAVA || b == id::LAVA
}

/// `getMaterial()` of the liquid block `b`.
fn liquid_mat(b: u16) -> u8 {
    if is_lava_block(b) {
        material::LAVA
    } else {
        material::WATER
    }
}

/// `BlockLiquid.getFlowingBlock(material)`
fn flowing_of(b: u16) -> u16 {
    if is_lava_block(b) {
        id::FLOWING_LAVA
    } else {
        id::FLOWING_WATER
    }
}

/// `BlockLiquid.getStaticBlock(material)`
fn static_of(b: u16) -> u16 {
    if is_lava_block(b) {
        id::LAVA
    } else {
        id::WATER
    }
}

/// `BlockLiquid.tickRate(world)` in the overworld: water 5, lava 30.
fn tick_rate(b: u16) -> i32 {
    if is_lava_block(b) {
        30
    } else {
        5
    }
}

/// `BlockLiquid.getDepth(state)`
fn get_depth(mat: u8, s: u16) -> i32 {
    if state::mat(s) == mat {
        level(s)
    } else {
        -1
    }
}

/// `BlockLiquid.triggerMixEffects`: the sound pitch draws two floats from `World.rand`.
pub fn trigger_mix_effects(w: &mut PopWorld, _p: Pos) {
    let _ = w.rand.next_float() - w.rand.next_float();
    // DROPPED: playSound and the 8 SMOKE_LARGE particles (Math.random, not World.rand).
}

/// `BlockLiquid.checkForMixing`
pub fn check_for_mixing(w: &mut PopWorld, p: Pos, s: u16) -> bool {
    if is_lava_block(blocks::block_of(s)) {
        let mut flag = false;
        for f in Facing::VALUES {
            if f != Facing::Down && state::mat(w.get(p.offset(f))) == material::WATER {
                flag = true;
                break;
            }
        }
        if flag {
            let integer = level(s);
            if integer == 0 {
                w.set3(p, blocks::state(id::OBSIDIAN, 0));
                trigger_mix_effects(w, p);
                return true;
            }
            if integer <= 4 {
                w.set3(p, blocks::state(id::COBBLESTONE, 0));
                trigger_mix_effects(w, p);
                return true;
            }
        }
    }
    false
}

/// `onBlockAdded`: `BlockDynamicLiquid` schedules itself, `BlockLiquid` (the still blocks) only mixes.
pub fn on_block_added(w: &mut PopWorld, p: Pos, s: u16) {
    let b = blocks::block_of(s);
    if b == id::FLOWING_WATER || b == id::FLOWING_LAVA {
        if !check_for_mixing(w, p, s) {
            w.schedule_update(p, b);
        }
    } else {
        check_for_mixing(w, p, s);
    }
}

/// `neighborChanged`: `BlockLiquid` mixes; `BlockStaticLiquid` also turns back into flowing liquid.
pub fn neighbor_changed(w: &mut PopWorld, p: Pos, s: u16) {
    let b = blocks::block_of(s);
    if b == id::FLOWING_WATER || b == id::FLOWING_LAVA {
        check_for_mixing(w, p, s);
    } else if !check_for_mixing(w, p, s) {
        update_liquid(w, p, s);
    }
}

/// `BlockStaticLiquid.updateLiquid`
fn update_liquid(w: &mut PopWorld, p: Pos, s: u16) {
    let flowing = flowing_of(blocks::block_of(s));
    w.set(p, blocks::state(flowing, s & 15), 2);
    // tickRate is the delay; only the immediate branch of updateBlockTick runs.
    let _ = tick_rate(flowing);
    w.schedule_update(p, flowing);
}

/// `updateTick`. `BlockStaticLiquid.updateTick` (lava setting fire) never runs: nothing schedules a
/// still liquid, `updateLiquid` schedules the flowing block.
pub fn update_tick(w: &mut PopWorld, p: Pos, s: u16, mut rand: TickRand) {
    let b = blocks::block_of(s);
    if b == id::FLOWING_WATER || b == id::FLOWING_LAVA {
        dynamic_update_tick(w, p, s, &mut rand);
    }
}

/// `BlockDynamicLiquid.updateTick`
fn dynamic_update_tick(w: &mut PopWorld, p: Pos, mut st: u16, rand: &mut TickRand) {
    let this = blocks::block_of(st);
    let lava = is_lava_block(this);
    let mat = liquid_mat(this);
    let mut i = level(st);
    // j: lava spreads 2 per block outside the Nether (doesWaterVaporize is false in the overworld).
    let j = if lava { 2 } else { 1 };
    let mut k = tick_rate(this);
    if i > 0 {
        let mut l = -100;
        let mut adjacent_source_blocks = 0;
        for f in Facing::HORIZONTAL {
            l = check_adjacent_block(w, mat, p.offset(f), l, &mut adjacent_source_blocks);
        }
        let mut i1 = l + j;
        if i1 >= 8 || l < 0 {
            i1 = -1;
        }
        let j1 = get_depth(mat, w.get(p.up()));
        if j1 >= 0 {
            if j1 >= 8 {
                i1 = j1;
            } else {
                i1 = j1 + 8;
            }
        }
        if adjacent_source_blocks >= 2 && mat == material::WATER {
            let below = w.get(p.down());
            if state::mat_solid(below) {
                i1 = 0;
            } else if state::mat(below) == mat && level(below) == 0 {
                i1 = 0;
            }
        }
        if lava && i < 8 && i1 < 8 && i1 > i && rand.next_int(w, 4) != 0 {
            k *= 4;
        }
        if i1 == i {
            place_static_block(w, p, st);
        } else {
            i = i1;
            if i1 < 0 {
                w.set_air(p);
            } else {
                st = blocks::state(this, i1 as u16);
                w.set(p, st, 2);
                let _ = k;
                w.schedule_update(p, this);
                w.notify_neighbors(p, this);
            }
        }
    } else {
        place_static_block(w, p, st);
    }

    let below_pos = p.down();
    let below = w.get(below_pos);
    if can_flow_into(w, mat, below_pos, below) {
        if lava && state::mat(w.get(below_pos)) == material::WATER {
            w.set3(below_pos, blocks::STONE);
            trigger_mix_effects(w, below_pos);
            return;
        }
        if i >= 8 {
            try_flow_into(w, this, below_pos, below, i);
        } else {
            try_flow_into(w, this, below_pos, below, i + 8);
        }
    } else if i >= 0 && (i == 0 || is_blocked(w, below_pos, below)) {
        let set = get_possible_flow_directions(w, mat, p);
        let mut k1 = i + j;
        if i >= 8 {
            k1 = 1;
        }
        if k1 >= 8 {
            return;
        }
        // EnumSet iterates in ordinal order: NORTH, SOUTH, WEST, EAST.
        for f in [Facing::North, Facing::South, Facing::West, Facing::East] {
            if set & (1 << f as u32) != 0 {
                let q = p.offset(f);
                let s = w.get(q);
                try_flow_into(w, this, q, s, k1);
            }
        }
    }
}

/// `BlockDynamicLiquid.placeStaticBlock`
fn place_static_block(w: &mut PopWorld, p: Pos, s: u16) {
    w.set(p, blocks::state(static_of(blocks::block_of(s)), s & 15), 2);
}

/// `BlockDynamicLiquid.tryFlowInto`
fn try_flow_into(w: &mut PopWorld, this: u16, p: Pos, s: u16, lvl: i32) {
    if can_flow_into(w, liquid_mat(this), p, s) {
        if state::mat(s) != material::AIR {
            if is_lava_block(this) {
                trigger_mix_effects(w, p);
            } else {
                behavior::drop_block_as_item(w, p, s);
            }
        }
        w.set(p, blocks::state(this, lvl as u16), 3);
    }
}

/// `BlockDynamicLiquid.getSlopeDistance`
fn get_slope_distance(w: &mut PopWorld, mat: u8, p: Pos, distance: i32, from: Facing) -> i32 {
    let mut i = 1000;
    for f in Facing::HORIZONTAL {
        if f != from {
            let bp = p.offset(f);
            let s = w.get(bp);
            if !is_blocked(w, bp, s) && (state::mat(s) != mat || level(s) > 0) {
                let down = bp.down();
                let ds = w.get(down);
                if !is_blocked(w, down, ds) {
                    return distance;
                }
                if distance < slope_find_distance(mat) {
                    let j = get_slope_distance(w, mat, bp, distance + 1, f.opposite());
                    if j < i {
                        i = j;
                    }
                }
            }
        }
    }
    i
}

/// `BlockDynamicLiquid.getSlopeFindDistance`: lava 2 outside the Nether, water 4.
fn slope_find_distance(mat: u8) -> i32 {
    if mat == material::LAVA {
        2
    } else {
        4
    }
}

/// `BlockDynamicLiquid.getPossibleFlowDirections`, as a bit set over `Facing` indices.
fn get_possible_flow_directions(w: &mut PopWorld, mat: u8, p: Pos) -> u32 {
    let mut i = 1000;
    let mut set = 0u32;
    for f in Facing::HORIZONTAL {
        let bp = p.offset(f);
        let s = w.get(bp);
        if !is_blocked(w, bp, s) && (state::mat(s) != mat || level(s) > 0) {
            let down = bp.down();
            let ds = w.get(down);
            let j = if is_blocked(w, down, ds) { get_slope_distance(w, mat, bp, 1, f.opposite()) } else { 0 };
            if j < i {
                set = 0;
            }
            if j <= i {
                set |= 1 << f as u32;
                i = j;
            }
        }
    }
    set
}

/// `BlockDynamicLiquid.isBlocked`: reads the block at `p` again and ignores the state it is given.
fn is_blocked(w: &mut PopWorld, p: Pos, _s: u16) -> bool {
    let s = w.get(p);
    let b = blocks::block_of(s);
    if matches!(b, id::WOODEN_DOOR | id::IRON_DOOR | id::STANDING_SIGN | id::LADDER | id::REEDS)
        || (id::SPRUCE_DOOR..=id::DARK_OAK_DOOR).contains(&b)
    {
        true
    } else {
        let m = state::mat(s);
        if m != material::PORTAL && m != material::STRUCTURE_VOID {
            state::blocks_movement(s)
        } else {
            true
        }
    }
}

/// `BlockDynamicLiquid.checkAdjacentBlock`
fn check_adjacent_block(w: &mut PopWorld, mat: u8, p: Pos, current: i32, sources: &mut i32) -> i32 {
    let mut i = get_depth(mat, w.get(p));
    if i < 0 {
        current
    } else {
        if i == 0 {
            *sources += 1;
        }
        if i >= 8 {
            i = 0;
        }
        if current >= 0 && i >= current {
            current
        } else {
            i
        }
    }
}

/// `BlockDynamicLiquid.canFlowInto`
fn can_flow_into(w: &mut PopWorld, mat: u8, p: Pos, s: u16) -> bool {
    let m = state::mat(s);
    m != mat && m != material::LAVA && !is_blocked(w, p, s)
}
