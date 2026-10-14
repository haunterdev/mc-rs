//! Block state properties from the generated [`props`](super::props) table, plus `BlockPos` and
//! `EnumFacing` as population uses them.

use super::props::{material, HARDNESS, PROPS};
use crate::java_random::JavaRandom;

#[inline(always)]
fn props(s: u16) -> u64 {
    PROPS[(s >> 4) as usize][(s & 15) as usize]
}

/// `getMaterial()`, as a [`material`] index.
#[inline(always)]
pub fn mat(s: u16) -> u8 {
    (props(s) & 63) as u8
}

#[inline(always)]
fn flag(s: u16, bit: u32) -> bool {
    props(s) >> 6 & (1 << bit) != 0
}

/// `Material.isSolid()`
pub fn mat_solid(s: u16) -> bool {
    flag(s, 0)
}
/// `Material.isLiquid()`
pub fn mat_liquid(s: u16) -> bool {
    flag(s, 1)
}
/// `Material.blocksMovement()`
pub fn blocks_movement(s: u16) -> bool {
    flag(s, 2)
}
/// `Material.isReplaceable()`
pub fn mat_replaceable(s: u16) -> bool {
    flag(s, 3)
}
/// `Material.isOpaque()`
pub fn mat_opaque(s: u16) -> bool {
    flag(s, 4)
}
/// `IBlockState.isFullBlock()`
pub fn is_full_block(s: u16) -> bool {
    flag(s, 7)
}
/// `IBlockState.isFullCube()`
pub fn is_full_cube(s: u16) -> bool {
    flag(s, 8)
}
/// `IBlockState.isOpaqueCube()`
pub fn is_opaque_cube(s: u16) -> bool {
    flag(s, 9)
}
/// `IBlockState.isTopSolid()`
pub fn is_top_solid(s: u16) -> bool {
    flag(s, 10)
}
/// `IBlockState.isNormalCube()`
pub fn is_normal_cube(s: u16) -> bool {
    flag(s, 11)
}
/// `Block.fullBlock`
pub fn full_block_field(s: u16) -> bool {
    flag(s, 12)
}
/// `Block.requiresUpdates()`
pub fn requires_updates(s: u16) -> bool {
    flag(s, 13)
}
/// `IBlockState.getLightOpacity()`
#[inline(always)]
pub fn opacity(s: u16) -> i32 {
    (props(s) >> 20 & 255) as i32
}
/// `IBlockState.getLightValue()`
pub fn light_value(s: u16) -> i32 {
    (props(s) >> 28 & 15) as i32
}
/// `getBlockFaceShape(world, pos, face) == BlockFaceShape.SOLID`
pub fn solid_face(s: u16, f: Facing) -> bool {
    props(s) >> (32 + f as u32) & 1 != 0
}
/// `IBlockState.getBlockHardness`
pub fn hardness(s: u16) -> f32 {
    HARDNESS[(s >> 4) as usize][(s & 15) as usize]
}
/// `Block.isBlockNormalCube(state)`: `material.blocksMovement() && isFullCube()`.
pub fn is_block_normal_cube(s: u16) -> bool {
    blocks_movement(s) && is_full_cube(s)
}
/// `Material.getCanBurn()`: set by `setBurning()` on WOOD, LEAVES, VINE, CLOTH, CARPET and TNT.
pub fn can_burn(s: u16) -> bool {
    matches!(mat(s), material::WOOD | material::LEAVES | material::VINE | material::CLOTH | material::CARPET | material::TNT)
}
pub fn is_mat_air(s: u16) -> bool {
    mat(s) == material::AIR
}

/// `EnumFacing`, in index order.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Facing {
    Down = 0,
    Up = 1,
    North = 2,
    South = 3,
    West = 4,
    East = 5,
}

impl Facing {
    pub const VALUES: [Facing; 6] = [Facing::Down, Facing::Up, Facing::North, Facing::South, Facing::West, Facing::East];
    /// `EnumFacing.Plane.HORIZONTAL` iteration order.
    pub const HORIZONTAL: [Facing; 4] = [Facing::North, Facing::East, Facing::South, Facing::West];
    /// `EnumFacing.HORIZONTALS`, by horizontal index.
    pub const BY_HORIZONTAL: [Facing; 4] = [Facing::South, Facing::West, Facing::North, Facing::East];

    pub fn offset(self) -> (i32, i32, i32) {
        match self {
            Facing::Down => (0, -1, 0),
            Facing::Up => (0, 1, 0),
            Facing::North => (0, 0, -1),
            Facing::South => (0, 0, 1),
            Facing::West => (-1, 0, 0),
            Facing::East => (1, 0, 0),
        }
    }
    pub fn opposite(self) -> Facing {
        match self {
            Facing::Down => Facing::Up,
            Facing::Up => Facing::Down,
            Facing::North => Facing::South,
            Facing::South => Facing::North,
            Facing::West => Facing::East,
            Facing::East => Facing::West,
        }
    }
    /// `getHorizontalIndex()`; -1 for the vertical facings.
    pub fn horizontal_index(self) -> i32 {
        match self {
            Facing::South => 0,
            Facing::West => 1,
            Facing::North => 2,
            Facing::East => 3,
            _ => -1,
        }
    }
    /// `rotateY()`: N, E, S, W clockwise.
    pub fn rotate_y(self) -> Facing {
        match self {
            Facing::North => Facing::East,
            Facing::East => Facing::South,
            Facing::South => Facing::West,
            Facing::West => Facing::North,
            f => panic!("rotateY on {f:?}"),
        }
    }
    /// `rotateYCCW()`
    pub fn rotate_y_ccw(self) -> Facing {
        match self {
            Facing::North => Facing::West,
            Facing::West => Facing::South,
            Facing::South => Facing::East,
            Facing::East => Facing::North,
            f => panic!("rotateYCCW on {f:?}"),
        }
    }
    pub fn is_horizontal(self) -> bool {
        !matches!(self, Facing::Down | Facing::Up)
    }
    /// `EnumFacing.random(rand)`
    pub fn random(rand: &mut JavaRandom) -> Facing {
        Facing::VALUES[rand.next_int_bounded(6) as usize]
    }
    /// `EnumFacing.Plane.HORIZONTAL.random(rand)`
    pub fn random_horizontal(rand: &mut JavaRandom) -> Facing {
        Facing::HORIZONTAL[rand.next_int_bounded(4) as usize]
    }
}

/// `BlockPos`
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct Pos {
    pub x: i32,
    pub y: i32,
    pub z: i32,
}

impl Pos {
    pub const fn new(x: i32, y: i32, z: i32) -> Pos {
        Pos { x, y, z }
    }
    pub fn add(self, dx: i32, dy: i32, dz: i32) -> Pos {
        Pos::new(self.x.wrapping_add(dx), self.y.wrapping_add(dy), self.z.wrapping_add(dz))
    }
    pub fn up(self) -> Pos {
        self.add(0, 1, 0)
    }
    pub fn up_n(self, n: i32) -> Pos {
        self.add(0, n, 0)
    }
    pub fn down(self) -> Pos {
        self.add(0, -1, 0)
    }
    pub fn down_n(self, n: i32) -> Pos {
        self.add(0, -n, 0)
    }
    pub fn north(self) -> Pos {
        self.add(0, 0, -1)
    }
    pub fn south(self) -> Pos {
        self.add(0, 0, 1)
    }
    pub fn west(self) -> Pos {
        self.add(-1, 0, 0)
    }
    pub fn east(self) -> Pos {
        self.add(1, 0, 0)
    }
    pub fn offset(self, f: Facing) -> Pos {
        let (dx, dy, dz) = f.offset();
        self.add(dx, dy, dz)
    }
    pub fn offset_n(self, f: Facing, n: i32) -> Pos {
        let (dx, dy, dz) = f.offset();
        self.add(dx * n, dy * n, dz * n)
    }
    /// `Vec3i.distanceSq(Vec3i)`
    pub fn distance_sq(self, o: Pos) -> f64 {
        let dx = (self.x - o.x) as f64;
        let dy = (self.y - o.y) as f64;
        let dz = (self.z - o.z) as f64;
        dx * dx + dy * dy + dz * dz
    }
}
