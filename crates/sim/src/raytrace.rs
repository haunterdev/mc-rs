//! What a ray hits: `World.rayTraceBlocks` through the blocks' `collisionRayTrace` and bounding
//! boxes, and the selection box `RenderGlobal.drawSelectionBox` outlines. Moved from the viewer's
//! target.rs.

use glam::DVec3;
use worldgen::blocks::id;
use worldgen::populate::behavior::torch_facing;
use worldgen::populate::state as st;
use worldgen::populate::Facing;

use crate::collision::{aabb, floor, Aabb, BlockAccess, CHEST, COCOA, FULL, LILY_PAD};
use crate::region::{self, props, Mat};

pub type BlockPos = (i32, i32, i32);

/// `BlockPos.offset(EnumFacing)`
pub fn offset(p: BlockPos, f: Facing) -> BlockPos {
    let (dx, dy, dz) = f.offset();
    (p.0 + dx, p.1 + dy, p.2 + dz)
}

/// `RayTraceResult.Type` without `ENTITY`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Miss,
    Block,
}

/// `RayTraceResult`: `hitVec`, `sideHit` and the block.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Hit {
    pub kind: Kind,
    pub vec: DVec3,
    pub face: Facing,
    pub pos: BlockPos,
}

/// `BlockBush.BUSH_AABB`
const BUSH: Aabb = aabb(0.30000001192092896, 0.0, 0.30000001192092896, 0.699999988079071, 0.6000000238418579, 0.699999988079071);
/// `BlockTallGrass.TALL_GRASS_AABB`, `BlockDeadBush.DEAD_BUSH_AABB` and `BlockSapling.SAPLING_AABB`
const TALL_GRASS: Aabb = aabb(0.09999999403953552, 0.0, 0.09999999403953552, 0.8999999761581421, 0.800000011920929, 0.8999999761581421);
/// `BlockMushroom.MUSHROOM_AABB`
const MUSHROOM: Aabb = aabb(0.30000001192092896, 0.0, 0.30000001192092896, 0.699999988079071, 0.4000000059604645, 0.699999988079071);
/// `BlockReed.REED_AABB`
const REED: Aabb = aabb(0.125, 0.0, 0.125, 0.875, 1.0, 0.875);
/// `BlockFarmland.FARMLAND_AABB`
const FARMLAND: Aabb = aabb(0.0, 0.0, 0.0, 1.0, 0.9375, 1.0);
/// `BlockSign.SIGN_AABB` and `BlockWallSign.SIGN_{NORTH, SOUTH, WEST, EAST}_AABB`.
const SIGN_STANDING: Aabb = aabb(0.25, 0.0, 0.25, 0.75, 1.0, 0.75);
const SIGN_NORTH: Aabb = aabb(0.0, 0.28125, 0.875, 1.0, 0.78125, 1.0);
const SIGN_SOUTH: Aabb = aabb(0.0, 0.28125, 0.0, 1.0, 0.78125, 0.125);
const SIGN_WEST: Aabb = aabb(0.875, 0.28125, 0.0, 1.0, 0.78125, 1.0);
const SIGN_EAST: Aabb = aabb(0.0, 0.28125, 0.0, 0.125, 0.78125, 1.0);
/// `BlockCactus.CACTUS_AABB`, the selection box.
const CACTUS: Aabb = aabb(0.0625, 0.0, 0.0625, 0.9375, 1.0, 0.9375);
/// `BlockVine.{UP, NORTH, EAST, SOUTH, WEST}_AABB`
/// `BlockTorch.STANDING_AABB` and the wall boxes, by `BlockTorch.FACING`.
const TORCH_STANDING: Aabb = aabb(0.4000000059604645, 0.0, 0.4000000059604645, 0.6000000238418579, 0.6000000238418579, 0.6000000238418579);
const TORCH_NORTH: Aabb = aabb(0.3499999940395355, 0.20000000298023224, 0.699999988079071, 0.6499999761581421, 0.800000011920929, 1.0);
const TORCH_SOUTH: Aabb = aabb(0.3499999940395355, 0.20000000298023224, 0.0, 0.6499999761581421, 0.800000011920929, 0.30000001192092896);
const TORCH_WEST: Aabb = aabb(0.699999988079071, 0.20000000298023224, 0.3499999940395355, 1.0, 0.800000011920929, 0.6499999761581421);
const TORCH_EAST: Aabb = aabb(0.0, 0.20000000298023224, 0.3499999940395355, 0.30000001192092896, 0.800000011920929, 0.6499999761581421);
const VINE_UP: Aabb = aabb(0.0, 0.9375, 0.0, 1.0, 1.0, 1.0);
const VINE_NORTH: Aabb = aabb(0.0, 0.0, 0.0, 1.0, 1.0, 0.0625);
const VINE_EAST: Aabb = aabb(0.9375, 0.0, 0.0, 1.0, 1.0, 1.0);
const VINE_SOUTH: Aabb = aabb(0.0, 0.0, 0.9375, 1.0, 1.0, 1.0);
const VINE_WEST: Aabb = aabb(0.0, 0.0, 0.0, 0.0625, 1.0, 1.0);

/// `BlockVine.getBoundingBox` over `getActualState`: the one side's box when exactly one is set,
/// else the full block. UP is solid ground above (meta bits S1 W2 N4 E8).
fn vine_box(w: &impl BlockAccess, s: u16, p: BlockPos) -> Aabb {
    let up = st::solid_face(w.state(p.0, p.1 + 1, p.2), Facing::Down);
    let sides = [(up, VINE_UP), (s & 4 != 0, VINE_NORTH), (s & 8 != 0, VINE_EAST), (s & 1 != 0, VINE_SOUTH), (s & 2 != 0, VINE_WEST)];
    let mut set = sides.iter().filter(|(on, _)| *on);
    match (set.next(), set.next()) {
        (Some((_, b)), None) => *b,
        _ => FULL,
    }
}

/// `IBlockState.getBoundingBox`, block-local.
/// DROPPED: the boxes of the blocks the generator does not place.
pub fn bounding_box(w: &impl BlockAccess, s: u16, p: BlockPos) -> Aabb {
    match s >> 4 {
        // BlockFlower: the bush box moved by Block.getOffset.
        id::YELLOW_FLOWER | id::RED_FLOWER => {
            let [x, y, z] = region::offset(s, p.0, p.2);
            BUSH.offset(x, y, z)
        }
        id::SAPLING | id::TALLGRASS | id::DEADBUSH => TALL_GRASS,
        id::BROWN_MUSHROOM | id::RED_MUSHROOM => MUSHROOM,
        id::REEDS => REED,
        id::WATERLILY => LILY_PAD,
        // BlockSnow.SNOW_AABB[layers], layers = meta + 1.
        id::SNOW_LAYER => Aabb { max_y: ((s & 7) + 1) as f64 * 0.125, ..FULL },
        id::VINE => vine_box(w, s, p),
        id::STONE_SLAB | id::WOODEN_SLAB | id::STONE_SLAB2 | id::PURPUR_SLAB => {
            if s & 8 == 0 {
                Aabb { max_y: 0.5, ..FULL }
            } else {
                Aabb { min_y: 0.5, ..FULL }
            }
        }
        id::COCOA => COCOA[(s & 3) as usize][((s & 15) >> 2).min(2) as usize],
        // DROPPED: the connected-chest boxes, as in collision.rs.
        id::CHEST => CHEST,
        id::FARMLAND => FARMLAND,
        // BlockSign.STANDING_AABB
        id::STANDING_SIGN => SIGN_STANDING,
        // BlockWallSign.getBoundingBox by FACING (meta), north for anything else.
        id::WALL_SIGN => match s & 15 {
            3 => SIGN_SOUTH,
            4 => SIGN_WEST,
            5 => SIGN_EAST,
            _ => SIGN_NORTH,
        },
        // BlockTorch.getBoundingBox
        id::TORCH => match torch_facing(s) {
            Facing::East => TORCH_EAST,
            Facing::West => TORCH_WEST,
            Facing::South => TORCH_SOUTH,
            Facing::North => TORCH_NORTH,
            _ => TORCH_STANDING,
        },
        b if worldgen::blocks::is_door(b) => crate::door::bounding_box(|x, y, z| w.state(x, y, z), s, p.0, p.1, p.2),
        _ => FULL,
    }
}

/// `IBlockState.getSelectedBoundingBox`, in world coordinates.
pub fn selected_box(w: &impl BlockAccess, s: u16, p: BlockPos) -> Aabb {
    let b = if s >> 4 == id::CACTUS { CACTUS } else { bounding_box(w, s, p) };
    b.offset(p.0 as f64, p.1 as f64, p.2 as f64)
}

/// `Block.canCollideCheck`: air and fire never; a liquid only its source, and only when asked.
fn can_collide_check(s: u16, stop_on_liquid: bool) -> bool {
    match props(s).mat {
        Mat::Air => false,
        Mat::Water | Mat::Lava => stop_on_liquid && s & 15 == 0,
        _ => s >> 4 != id::FIRE,
    }
}

/// `Vec3d.getIntermediateWith{X,Y,Z}Value`
fn intermediate(a: DVec3, b: DVec3, axis: usize, v: f64) -> Option<DVec3> {
    let d = b - a;
    if d[axis] * d[axis] < 1.0000000116860974E-7 {
        return None;
    }
    let t = (v - a[axis]) / d[axis];
    (0.0..=1.0).contains(&t).then(|| DVec3::new(a.x + d.x * t, a.y + d.y * t, a.z + d.z * t))
}

/// `AxisAlignedBB.calculateIntercept`: the nearest face the segment crosses, checked in the order
/// minX, maxX, minY, maxY, minZ, maxZ, a later one winning only when strictly nearer.
pub fn calculate_intercept(bb: &Aabb, a: DVec3, b: DVec3) -> Option<(DVec3, Facing)> {
    let planes = [
        (0, bb.min_x, Facing::West),
        (0, bb.max_x, Facing::East),
        (1, bb.min_y, Facing::Down),
        (1, bb.max_y, Facing::Up),
        (2, bb.min_z, Facing::North),
        (2, bb.max_z, Facing::South),
    ];
    let mut best: Option<(DVec3, Facing)> = None;
    for (axis, v, face) in planes {
        let Some(p) = intermediate(a, b, axis, v) else { continue };
        // intersectsWithYZ, XZ, XY: inclusive.
        let inside = match axis {
            0 => p.y >= bb.min_y && p.y <= bb.max_y && p.z >= bb.min_z && p.z <= bb.max_z,
            1 => p.x >= bb.min_x && p.x <= bb.max_x && p.z >= bb.min_z && p.z <= bb.max_z,
            _ => p.x >= bb.min_x && p.x <= bb.max_x && p.y >= bb.min_y && p.y <= bb.max_y,
        };
        if inside && best.is_none_or(|(q, _)| a.distance_squared(p) < a.distance_squared(q)) {
            best = Some((p, face));
        }
    }
    best
}

/// `Block.collisionRayTrace` through `Block.rayTrace` with the bounding box.
fn collision_ray_trace(w: &impl BlockAccess, s: u16, p: BlockPos, start: DVec3, end: DVec3) -> Option<Hit> {
    let o = DVec3::new(p.0 as f64, p.1 as f64, p.2 as f64);
    calculate_intercept(&bounding_box(w, s, p), start - o, end - o).map(|(v, face)| Hit { kind: Kind::Block, vec: v + o, face, pos: p })
}

/// `World.rayTraceBlocks(start, end, stopOnLiquid, false, returnLastUncollidableBlock)`: walks
/// the blocks the segment passes, at most 200 steps.
/// DROPPED: `ignoreBlockWithoutBoundingBox`; every caller here passes false.
pub fn ray_trace_blocks(w: &impl BlockAccess, mut start: DVec3, end: DVec3, stop_on_liquid: bool, return_last: bool) -> Option<Hit> {
    if start.is_nan() || end.is_nan() {
        return None;
    }
    let (i, j, k) = (floor(end.x), floor(end.y), floor(end.z));
    let (mut l, mut i1, mut j1) = (floor(start.x), floor(start.y), floor(start.z));
    let s = w.state(l, i1, j1);
    if can_collide_check(s, stop_on_liquid) {
        if let Some(h) = collision_ray_trace(w, s, (l, i1, j1), start, end) {
            return Some(h);
        }
    }
    let mut last = None;
    // while (k1-- >= 0), from 200.
    for _ in 0..=200 {
        if start.is_nan() {
            return None;
        }
        if l == i && i1 == j && j1 == k {
            return if return_last { last } else { None };
        }
        let (mut flag2, mut flag, mut flag1) = (true, true, true);
        let (mut d0, mut d1, mut d2) = (999.0, 999.0, 999.0);
        if i > l {
            d0 = l as f64 + 1.0;
        } else if i < l {
            d0 = l as f64 + 0.0;
        } else {
            flag2 = false;
        }
        if j > i1 {
            d1 = i1 as f64 + 1.0;
        } else if j < i1 {
            d1 = i1 as f64 + 0.0;
        } else {
            flag = false;
        }
        if k > j1 {
            d2 = j1 as f64 + 1.0;
        } else if k < j1 {
            d2 = j1 as f64 + 0.0;
        } else {
            flag1 = false;
        }
        let (mut d3, mut d4, mut d5) = (999.0, 999.0, 999.0);
        let d6 = end.x - start.x;
        let d7 = end.y - start.y;
        let d8 = end.z - start.z;
        if flag2 {
            d3 = (d0 - start.x) / d6;
        }
        if flag {
            d4 = (d1 - start.y) / d7;
        }
        if flag1 {
            d5 = (d2 - start.z) / d8;
        }
        // `d == -0.0D` holds for either zero.
        if d3 == 0.0 {
            d3 = -1.0E-4;
        }
        if d4 == 0.0 {
            d4 = -1.0E-4;
        }
        if d5 == 0.0 {
            d5 = -1.0E-4;
        }
        let face;
        if d3 < d4 && d3 < d5 {
            face = if i > l { Facing::West } else { Facing::East };
            start = DVec3::new(d0, start.y + d7 * d3, start.z + d8 * d3);
        } else if d4 < d5 {
            face = if j > i1 { Facing::Down } else { Facing::Up };
            start = DVec3::new(start.x + d6 * d4, d1, start.z + d8 * d4);
        } else {
            face = if k > j1 { Facing::North } else { Facing::South };
            start = DVec3::new(start.x + d6 * d5, start.y + d7 * d5, d2);
        }
        l = floor(start.x) - (face == Facing::East) as i32;
        i1 = floor(start.y) - (face == Facing::Up) as i32;
        j1 = floor(start.z) - (face == Facing::South) as i32;
        let s = w.state(l, i1, j1);
        if can_collide_check(s, stop_on_liquid) {
            if let Some(h) = collision_ray_trace(w, s, (l, i1, j1), start, end) {
                return Some(h);
            }
        } else {
            last = Some(Hit { kind: Kind::Miss, vec: start, face, pos: (l, i1, j1) });
        }
    }
    if return_last {
        last
    } else {
        None
    }
}

/// `PlayerControllerMP.getBlockReachDistance` in creative.
pub const REACH: f64 = 5.0;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::Blocks;
    use worldgen::blocks::{state, STONE, WATER};

    #[test]
    fn the_ray_stops_at_the_first_face() {
        let w = Blocks::floor_at(63, 8);
        // Straight down from 1.62 over the floor.
        let h = ray_trace_blocks(&w, DVec3::new(0.5, 65.62, 0.5), DVec3::new(0.5, 60.0, 0.5), false, true).unwrap();
        assert_eq!((h.kind, h.pos, h.face), (Kind::Block, (0, 63, 0), Facing::Up));
        assert_eq!(h.vec.y, 64.0);
        // Along +x into a wall: its west face.
        let mut w = Blocks::default();
        w.set(3, 64, 0, STONE);
        let h = ray_trace_blocks(&w, DVec3::new(0.5, 64.5, 0.5), DVec3::new(5.5, 64.5, 0.5), false, true).unwrap();
        assert_eq!((h.pos, h.face), ((3, 64, 0), Facing::West));
        assert_eq!(h.vec.x, 3.0);
        // Nothing in reach: the last block passed, as a miss, by the face it was entered through.
        let h = ray_trace_blocks(&w, DVec3::new(0.5, 70.5, 0.5), DVec3::new(0.5, 70.5, 4.5), false, true).unwrap();
        assert_eq!((h.kind, h.pos, h.face), (Kind::Miss, (0, 70, 4), Facing::North));
        assert!(ray_trace_blocks(&w, DVec3::new(0.5, 70.5, 0.5), DVec3::new(0.5, 70.5, 4.5), false, false).is_none());
    }

    #[test]
    fn liquids_are_seen_only_when_asked() {
        let mut w = Blocks::floor_at(61, 4);
        for x in -4..=4 {
            for z in -4..=4 {
                w.set(x, 62, z, WATER);
                w.set(x, 63, z, WATER);
            }
        }
        let (a, b) = (DVec3::new(0.5, 66.0, 0.5), DVec3::new(0.5, 61.0, 0.5));
        assert_eq!(ray_trace_blocks(&w, a, b, false, true).unwrap().pos, (0, 61, 0));
        let h = ray_trace_blocks(&w, a, b, true, false).unwrap();
        assert_eq!((h.pos, h.face, h.vec.y), ((0, 63, 0), Facing::Up, 64.0));
    }

    #[test]
    fn small_boxes_let_the_ray_pass() {
        // A bottom slab: a ray at y 0.75 over it goes on to the stone behind.
        let mut w = Blocks::default();
        w.set(2, 64, 0, state(id::STONE_SLAB, 0));
        w.set(3, 64, 0, STONE);
        let h = ray_trace_blocks(&w, DVec3::new(0.5, 64.75, 0.5), DVec3::new(5.5, 64.75, 0.5), false, true).unwrap();
        assert_eq!(h.pos, (3, 64, 0));
        let h = ray_trace_blocks(&w, DVec3::new(0.5, 64.25, 0.5), DVec3::new(5.5, 64.25, 0.5), false, true).unwrap();
        assert_eq!((h.pos, h.face), ((2, 64, 0), Facing::West));
        // A vine on one wall is its thin side box; the selection box of a cactus is inset.
        w.set(5, 64, 0, state(id::VINE, 8));
        assert_eq!(bounding_box(&w, state(id::VINE, 8), (5, 64, 0)), VINE_EAST);
        assert_eq!(bounding_box(&w, state(id::VINE, 9), (5, 64, 0)), FULL);
        assert_eq!(selected_box(&w, state(id::CACTUS, 0), (0, 0, 0)), CACTUS);
    }
}
