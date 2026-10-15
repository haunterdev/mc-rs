//! `BlockDoor`: the two halves' metadata (`getStateFromMeta`, `getMetaFromState`),
//! `getActualState` and `getBoundingBox`, shared by the viewer's mesher, ray trace, collision and
//! placement.
//!
//! The lower half's meta is FACING (`rotateY().getHorizontalIndex()`) and OPEN (4); the upper
//! half's is 8, HINGE right (1) and POWERED (2). Each half reads the rest from the other.

use worldgen::blocks::{block_of, state};
use worldgen::populate::Facing;

use crate::collision::{aabb, Aabb};

pub const SOUTH_AABB: Aabb = aabb(0.0, 0.0, 0.0, 1.0, 1.0, 0.1875);
pub const NORTH_AABB: Aabb = aabb(0.0, 0.0, 0.8125, 1.0, 1.0, 1.0);
pub const WEST_AABB: Aabb = aabb(0.8125, 0.0, 0.0, 1.0, 1.0, 1.0);
pub const EAST_AABB: Aabb = aabb(0.0, 0.0, 0.0, 0.1875, 1.0, 1.0);

/// `getStateFromMeta`'s FACING for a lower half's meta: `byHorizontalIndex(meta & 3).rotateYCCW()`.
pub fn facing_of(meta: u16) -> Facing {
    Facing::BY_HORIZONTAL[(meta & 3) as usize].rotate_y_ccw()
}

/// `getMetaFromState` for the lower half.
pub fn lower_meta(facing: Facing, open: bool) -> u16 {
    facing.rotate_y().horizontal_index() as u16 | if open { 4 } else { 0 }
}

/// `getMetaFromState` for the upper half.
pub fn upper_meta(right_hinge: bool, powered: bool) -> u16 {
    8 | right_hinge as u16 | (powered as u16) << 1
}

/// `getActualState` folded into the four bits the models are keyed by, plus HINGE: a lower half
/// keeps its own meta and takes HINGE from the upper half above; an upper half becomes 8 with
/// the lower half's FACING and OPEN bits and keeps its own HINGE. A half without its partner
/// keeps the defaults (FACING north, OPEN false, HINGE left). POWERED is left out; the models
/// ignore it.
pub fn actual(get: impl Fn(i32, i32, i32) -> u16, s: u16, x: i32, y: i32, z: i32) -> (u16, bool) {
    let b = block_of(s);
    if s & 8 == 0 {
        let above = get(x, y + 1, z);
        (s, block_of(above) == b && above & 8 != 0 && above & 1 != 0)
    } else {
        let below = get(x, y - 1, z);
        let low = if block_of(below) == b && below & 8 == 0 { below & 7 } else { lower_meta(Facing::North, false) };
        (state(b, 8 | low), s & 1 != 0)
    }
}

/// `BlockDoor.getBoundingBox` over `getActualState`: the closed door's slab on its FACING side,
/// the open one swung to the hinge side.
pub fn bounding_box(get: impl Fn(i32, i32, i32) -> u16, s: u16, x: i32, y: i32, z: i32) -> Aabb {
    let (a, right) = actual(get, s, x, y, z);
    let closed = a & 4 == 0;
    match facing_of(a) {
        Facing::South => {
            if closed {
                SOUTH_AABB
            } else if right {
                EAST_AABB
            } else {
                WEST_AABB
            }
        }
        Facing::West => {
            if closed {
                WEST_AABB
            } else if right {
                SOUTH_AABB
            } else {
                NORTH_AABB
            }
        }
        Facing::North => {
            if closed {
                NORTH_AABB
            } else if right {
                WEST_AABB
            } else {
                EAST_AABB
            }
        }
        _ => {
            if closed {
                EAST_AABB
            } else if right {
                NORTH_AABB
            } else {
                SOUTH_AABB
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use worldgen::blocks::id;

    #[test]
    fn metadata_round_trips_and_halves_share_state() {
        for f in Facing::HORIZONTAL {
            for open in [false, true] {
                let m = lower_meta(f, open);
                assert_eq!(facing_of(m), f);
                assert_eq!(m & 4 != 0, open);
            }
        }
        // East-facing lower half (meta 0) with a right-hinged upper half above it.
        let lower = state(id::WOODEN_DOOR, lower_meta(Facing::East, true));
        let upper = state(id::WOODEN_DOOR, upper_meta(true, false));
        let get = |_: i32, y: i32, _: i32| if y == 64 { lower } else if y == 65 { upper } else { 0 };
        assert_eq!(actual(get, lower, 0, 64, 0), (lower, true));
        assert_eq!(actual(get, upper, 0, 65, 0), (state(id::WOODEN_DOOR, 8 | (lower & 7)), true));
        // Open, east, right hinge: BlockDoor's NORTH_AABB, on both halves.
        assert_eq!(bounding_box(get, lower, 0, 64, 0), NORTH_AABB);
        assert_eq!(bounding_box(get, upper, 0, 65, 0), NORTH_AABB);
        // A lone upper half: north, closed.
        assert_eq!(bounding_box(|_, _, _| 0, upper, 0, 65, 0), NORTH_AABB);
        assert_eq!(facing_of(actual(|_, _, _| 0, upper, 0, 65, 0).0), Facing::North);
    }
}
