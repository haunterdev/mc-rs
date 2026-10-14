//! Block collision for the player: `AxisAlignedBB`, the boxes `Block.addCollisionBoxToList` adds
//! for the blocks the generator places, and the `World` queries `Entity.move`, the fluid checks
//! and `EntityPlayerSP` make.

use glam::DVec3;
use worldgen::blocks::id;

use crate::region::{liquid_height_percent, props, Mat};

/// `MathHelper.floor(double)`
pub fn floor(v: f64) -> i32 {
    v.floor() as i32
}

/// `MathHelper.ceil(double)`
pub fn ceil(v: f64) -> i32 {
    v.ceil() as i32
}

/// `MathHelper.fastInvSqrt(double)`
pub fn fast_inv_sqrt(v: f64) -> f64 {
    let d0 = 0.5 * v;
    let i = 6_910_469_410_427_058_090i64.wrapping_sub((v.to_bits() as i64) >> 1);
    let v = f64::from_bits(i as u64);
    v * (1.5 - d0 * v * v)
}

/// `AxisAlignedBB`
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Aabb {
    pub min_x: f64,
    pub min_y: f64,
    pub min_z: f64,
    pub max_x: f64,
    pub max_y: f64,
    pub max_z: f64,
}

impl Aabb {
    pub fn new(x1: f64, y1: f64, z1: f64, x2: f64, y2: f64, z2: f64) -> Aabb {
        Aabb { min_x: x1.min(x2), min_y: y1.min(y2), min_z: z1.min(z2), max_x: x1.max(x2), max_y: y1.max(y2), max_z: z1.max(z2) }
    }

    /// `new AxisAlignedBB(Vec3d, Vec3d)`
    pub fn of(a: DVec3, b: DVec3) -> Aabb {
        Aabb::new(a.x, a.y, a.z, b.x, b.y, b.z)
    }

    pub fn offset(&self, x: f64, y: f64, z: f64) -> Aabb {
        Aabb::new(self.min_x + x, self.min_y + y, self.min_z + z, self.max_x + x, self.max_y + y, self.max_z + z)
    }

    /// Grows toward the sign of each component only.
    pub fn expand(&self, x: f64, y: f64, z: f64) -> Aabb {
        let (mut d0, mut d1, mut d2) = (self.min_x, self.min_y, self.min_z);
        let (mut d3, mut d4, mut d5) = (self.max_x, self.max_y, self.max_z);
        if x < 0.0 {
            d0 += x;
        } else if x > 0.0 {
            d3 += x;
        }
        if y < 0.0 {
            d1 += y;
        } else if y > 0.0 {
            d4 += y;
        }
        if z < 0.0 {
            d2 += z;
        } else if z > 0.0 {
            d5 += z;
        }
        Aabb::new(d0, d1, d2, d3, d4, d5)
    }

    pub fn grow(&self, x: f64, y: f64, z: f64) -> Aabb {
        Aabb::new(self.min_x - x, self.min_y - y, self.min_z - z, self.max_x + x, self.max_y + y, self.max_z + z)
    }

    pub fn shrink(&self, v: f64) -> Aabb {
        self.grow(-v, -v, -v)
    }

    /// How far `other` may move along x by `offset` before it touches this box.
    pub fn calculate_x_offset(&self, other: &Aabb, mut offset: f64) -> f64 {
        if other.max_y > self.min_y && other.min_y < self.max_y && other.max_z > self.min_z && other.min_z < self.max_z {
            if offset > 0.0 && other.max_x <= self.min_x {
                let d1 = self.min_x - other.max_x;
                if d1 < offset {
                    offset = d1;
                }
            } else if offset < 0.0 && other.min_x >= self.max_x {
                let d0 = self.max_x - other.min_x;
                if d0 > offset {
                    offset = d0;
                }
            }
        }
        offset
    }

    pub fn calculate_y_offset(&self, other: &Aabb, mut offset: f64) -> f64 {
        if other.max_x > self.min_x && other.min_x < self.max_x && other.max_z > self.min_z && other.min_z < self.max_z {
            if offset > 0.0 && other.max_y <= self.min_y {
                let d1 = self.min_y - other.max_y;
                if d1 < offset {
                    offset = d1;
                }
            } else if offset < 0.0 && other.min_y >= self.max_y {
                let d0 = self.max_y - other.min_y;
                if d0 > offset {
                    offset = d0;
                }
            }
        }
        offset
    }

    pub fn calculate_z_offset(&self, other: &Aabb, mut offset: f64) -> f64 {
        if other.max_x > self.min_x && other.min_x < self.max_x && other.max_y > self.min_y && other.min_y < self.max_y {
            if offset > 0.0 && other.max_z <= self.min_z {
                let d1 = self.min_z - other.max_z;
                if d1 < offset {
                    offset = d1;
                }
            } else if offset < 0.0 && other.min_z >= self.max_z {
                let d0 = self.max_z - other.min_z;
                if d0 > offset {
                    offset = d0;
                }
            }
        }
        offset
    }

    pub fn intersects(&self, o: &Aabb) -> bool {
        self.min_x < o.max_x && self.max_x > o.min_x && self.min_y < o.max_y && self.max_y > o.min_y && self.min_z < o.max_z && self.max_z > o.min_z
    }

    /// `intersects(Vec3d min, Vec3d max)`: the box the two points span.
    pub fn intersects_span(&self, a: DVec3, b: DVec3) -> bool {
        self.intersects(&Aabb::of(a, b))
    }

    pub fn center(&self) -> DVec3 {
        DVec3::new(
            self.min_x + (self.max_x - self.min_x) * 0.5,
            self.min_y + (self.max_y - self.min_y) * 0.5,
            self.min_z + (self.max_z - self.min_z) * 0.5,
        )
    }
}

/// The world as the player's collision reads it.
pub trait BlockAccess {
    /// The block state; air in an unloaded chunk (the client's `EmptyChunk`) and outside 0..256.
    fn state(&self, x: i32, y: i32, z: i32) -> u16;
    /// Whether the chunk holding the column is loaded (`Chunk.isLoaded`, for the gravity in
    /// `EntityLivingBase.travel`).
    fn loaded(&self, x: i32, z: i32) -> bool;
}

/// `Block.FULL_BLOCK_AABB`
pub const FULL: Aabb = Aabb { min_x: 0.0, min_y: 0.0, min_z: 0.0, max_x: 1.0, max_y: 1.0, max_z: 1.0 };
/// `BlockLilyPad.LILY_PAD_AABB`
pub const LILY_PAD: Aabb = Aabb { min_x: 0.0625, min_y: 0.0, min_z: 0.0625, max_x: 0.9375, max_y: 0.093_75, max_z: 0.9375 };

pub const fn aabb(min_x: f64, min_y: f64, min_z: f64, max_x: f64, max_y: f64, max_z: f64) -> Aabb {
    Aabb { min_x, min_y, min_z, max_x, max_y, max_z }
}

/// `BlockCactus.CACTUS_COLLISION_AABB`
const CACTUS: Aabb = aabb(0.0625, 0.0, 0.0625, 0.9375, 0.9375, 0.9375);
/// `BlockChest.NOT_CONNECTED_AABB`
pub const CHEST: Aabb = aabb(0.0625, 0.0, 0.0625, 0.9375, 0.875, 0.9375);
/// `BlockCocoa.COCOA_{SOUTH, WEST, NORTH, EAST}_AABB`, by horizontal index, then by age.
pub const COCOA: [[Aabb; 3]; 4] = [
    [
        aabb(0.375, 0.4375, 0.6875, 0.625, 0.75, 0.9375),
        aabb(0.3125, 0.3125, 0.5625, 0.6875, 0.75, 0.9375),
        aabb(0.25, 0.1875, 0.4375, 0.75, 0.75, 0.9375),
    ],
    [
        aabb(0.0625, 0.4375, 0.375, 0.3125, 0.75, 0.625),
        aabb(0.0625, 0.3125, 0.3125, 0.4375, 0.75, 0.6875),
        aabb(0.0625, 0.1875, 0.25, 0.5625, 0.75, 0.75),
    ],
    [
        aabb(0.375, 0.4375, 0.0625, 0.625, 0.75, 0.3125),
        aabb(0.3125, 0.3125, 0.0625, 0.6875, 0.75, 0.4375),
        aabb(0.25, 0.1875, 0.0625, 0.75, 0.75, 0.5625),
    ],
    [
        aabb(0.6875, 0.4375, 0.375, 0.9375, 0.75, 0.625),
        aabb(0.5625, 0.3125, 0.3125, 0.9375, 0.75, 0.6875),
        aabb(0.4375, 0.1875, 0.25, 0.9375, 0.75, 0.75),
    ],
];

/// `IBlockState.getCollisionBoundingBox`, in block-local coordinates.
pub fn collision_box(state: u16) -> Option<Aabb> {
    match state >> 4 {
        // BlockSnow.SNOW_AABB[layers - 1], with layers = meta + 1.
        id::SNOW_LAYER => Some(Aabb { max_y: (state & 7) as f64 * 0.125, ..FULL }),
        // BlockLilyPad has none (NULL_AABB); addCollisionBoxToList adds its own box instead.
        id::WATERLILY => None,
        id::CACTUS => Some(CACTUS),
        // BlockSlab.AABB_BOTTOM_HALF and AABB_TOP_HALF by HALF (meta & 8).
        id::STONE_SLAB | id::WOODEN_SLAB | id::STONE_SLAB2 | id::PURPUR_SLAB => {
            Some(if state & 8 == 0 { Aabb { max_y: 0.5, ..FULL } } else { Aabb { min_y: 0.5, ..FULL } })
        }
        // DROPPED: the connected-chest boxes (BlockChest.getBoundingBox reads the neighbors);
        // dungeon chests stand alone.
        id::CHEST => Some(CHEST),
        // BlockCocoa: FACING byHorizontalIndex(meta & 3), AGE (meta & 15) >> 2.
        id::COCOA => Some(COCOA[(state & 3) as usize][((state & 15) >> 2).min(2) as usize]),
        _ => match props(state).mat {
            Mat::Air | Mat::Water | Mat::Lava | Mat::Logic => None,
            Mat::Ice | Mat::Solid => Some(FULL),
        },
    }
}

/// `IBlockState.addCollisionBoxToList`: the block's box at `pos` when it meets `entity_box`.
fn add_collision_boxes(state: u16, x: i32, y: i32, z: i32, entity_box: &Aabb, out: &mut Vec<Aabb>) {
    let local = if state >> 4 == id::WATERLILY { Some(LILY_PAD) } else { collision_box(state) };
    if let Some(b) = local {
        let b = b.offset(x as f64, y as f64, z as f64);
        if entity_box.intersects(&b) {
            out.push(b);
        }
    }
}

/// `World.getCollisionBoxes(entity, aabb, flag, list)` over blocks only.
/// DROPPED: the world border (the stone it stands in for outside it) and the 30 000 000 limit;
/// `isBlockLoaded` is always true on the client, and an unloaded chunk reads as air.
fn collision_boxes(w: &impl BlockAccess, bb: &Aabb, first_only: bool, out: &mut Vec<Aabb>) -> bool {
    let i = floor(bb.min_x) - 1;
    let j = ceil(bb.max_x) + 1;
    let k = floor(bb.min_y) - 1;
    let l = ceil(bb.max_y) + 1;
    let i1 = floor(bb.min_z) - 1;
    let j1 = ceil(bb.max_z) + 1;
    for k1 in i..j {
        for l1 in i1..j1 {
            let flag2 = k1 == i || k1 == j - 1;
            let flag3 = l1 == i1 || l1 == j1 - 1;
            if flag2 && flag3 {
                continue;
            }
            for i2 in k..l {
                if !flag2 && !flag3 || i2 != l - 1 {
                    add_collision_boxes(w.state(k1, i2, l1), k1, i2, l1, bb, out);
                    if first_only && !out.is_empty() {
                        return true;
                    }
                }
            }
        }
    }
    !out.is_empty()
}

/// `World.getCollisionBoxes(entity, aabb)`.
/// DROPPED: the entities' boxes; the world has no entities.
pub fn get_collision_boxes(w: &impl BlockAccess, bb: &Aabb) -> Vec<Aabb> {
    let mut out = Vec::new();
    collision_boxes(w, bb, false, &mut out);
    out
}

/// `World.collidesWithAnyBlock`
pub fn collides_with_any_block(w: &impl BlockAccess, bb: &Aabb) -> bool {
    collision_boxes(w, bb, true, &mut Vec::new())
}

/// The block positions `World.containsAnyLiquid` and its relatives walk.
fn cells(bb: &Aabb) -> impl Iterator<Item = (i32, i32, i32)> {
    let (j2, k2, l2, i3, j3, k3) = (floor(bb.min_x), ceil(bb.max_x), floor(bb.min_y), ceil(bb.max_y), floor(bb.min_z), ceil(bb.max_z));
    (j2..k2).flat_map(move |x| (l2..i3).flat_map(move |y| (j3..k3).map(move |z| (x, y, z))))
}

fn is_liquid(state: u16) -> bool {
    matches!(props(state).mat, Mat::Water | Mat::Lava)
}

/// `World.containsAnyLiquid`
pub fn contains_any_liquid(w: &impl BlockAccess, bb: &Aabb) -> bool {
    cells(bb).any(|(x, y, z)| is_liquid(w.state(x, y, z)))
}

/// `World.isMaterialInBB`
pub fn is_material_in_bb(w: &impl BlockAccess, bb: &Aabb, mat: Mat) -> bool {
    cells(bb).any(|(x, y, z)| props(w.state(x, y, z)).mat == mat)
}

/// `World.handleMaterialAcceleration`: whether the box reaches into the material's surface.
/// DROPPED: the flow push. The generator places only still sources (level 0), whose flow vector
/// is zero.
pub fn handle_material_acceleration(w: &impl BlockAccess, bb: &Aabb, mat: Mat) -> bool {
    let i3 = ceil(bb.max_y);
    // isAreaLoaded: on the client only the height range can fail.
    if !(i3 >= 0 && floor(bb.min_y) < 256) {
        return false;
    }
    let mut flag = false;
    for (x, y, z) in cells(bb) {
        let s = w.state(x, y, z);
        if props(s).mat == mat {
            let d0 = ((y + 1) as f32 - liquid_height_percent((s & 15) as i32)) as f64;
            if i3 as f64 >= d0 {
                flag = true;
            }
        }
    }
    flag
}

/// `Block.slipperiness` of the state.
pub fn slipperiness(state: u16) -> f32 {
    // BlockIce and BlockPackedIce (174) set 0.98; every other placed block keeps 0.6.
    match state >> 4 {
        id::ICE | 174 => 0.98,
        _ => 0.6,
    }
}

/// `IBlockState.isNormalCube`: an opaque material (not ice, not liquid, not a plant) on a full
/// cube.
pub fn is_normal_cube(state: u16) -> bool {
    let p = props(state);
    p.mat == Mat::Solid && p.full_cube
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::testing::Blocks;
    use worldgen::blocks::{STONE, WATER, WATERLILY};

    #[test]
    fn offsets_stop_at_the_face() {
        let block = Aabb::new(0.0, 0.0, 0.0, 1.0, 1.0, 1.0);
        let player = Aabb::new(-0.7, 0.0, 0.2, -0.1, 1.8, 0.8);
        assert_eq!(block.calculate_x_offset(&player, 0.5), 0.1);
        assert_eq!(block.calculate_x_offset(&player, -0.5), -0.5);
        // Above the block it does not touch.
        assert_eq!(block.calculate_x_offset(&player.offset(0.0, 1.0, 0.0), 0.5), 0.5);
        let above = Aabb::new(0.2, 1.5, 0.2, 0.8, 3.3, 0.8);
        assert_eq!(block.calculate_y_offset(&above, -1.0), -0.5);
        assert_eq!(Aabb::new(1.0, 2.0, 3.0, 0.0, 0.0, 0.0), Aabb::new(0.0, 0.0, 0.0, 1.0, 2.0, 3.0));
        assert_eq!(block.expand(-0.5, 0.0, 2.0), Aabb::new(-0.5, 0.0, 0.0, 1.0, 1.0, 3.0));
    }

    #[test]
    fn boxes_skip_corner_columns_and_edge_tops() {
        let mut w = Blocks::default();
        let bb = Aabb::new(0.2, 1.0, 0.2, 0.8, 2.8, 0.8);
        // The scan runs x and z over -1..2 and y over 0..4; corners and the top of the edges are
        // skipped, and only boxes that meet the entity box count.
        w.set(0, 0, 0, STONE);
        w.set(0, 1, 0, STONE);
        assert_eq!(get_collision_boxes(&w, &bb).len(), 1);
        assert!(collides_with_any_block(&w, &bb));
        // Resting exactly on top is not a collision: the test is strict.
        assert!(!collides_with_any_block(&w, &bb.offset(0.0, 1.0, 0.0)));
        // The lily pad has no getCollisionBoundingBox but still collides.
        assert!(collision_box(WATERLILY).is_none());
        let mut p = Blocks::default();
        p.set(0, 0, 0, WATERLILY);
        assert_eq!(get_collision_boxes(&p, &Aabb::new(0.2, 0.0, 0.2, 0.8, 1.8, 0.8))[0].max_y, 0.093_75);
    }

    #[test]
    fn water_surface_counts_from_the_level() {
        let mut w = Blocks::default();
        w.set(0, 0, 0, WATER);
        // A source fills 8/9 of the block: a box reaching y 0.5 rounds up to 1 and counts.
        let bb = Aabb::new(0.2, 0.4, 0.2, 0.8, 0.5, 0.8);
        assert!(handle_material_acceleration(&w, &bb, Mat::Water));
        assert!(contains_any_liquid(&w, &bb));
        assert!(!handle_material_acceleration(&w, &bb.offset(0.0, 1.0, 0.0), Mat::Water));
    }

    #[test]
    fn fast_inv_sqrt_is_close() {
        for v in [0.01, 0.5, 1.0, 4.0, 1234.5] {
            let want = 1.0 / f64::sqrt(v);
            assert!((fast_inv_sqrt(v) - want).abs() / want < 0.002, "{v}");
        }
    }
}
