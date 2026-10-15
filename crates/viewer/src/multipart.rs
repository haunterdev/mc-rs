//! `getActualState` of the multipart blocks (fences, panes, iron bars, walls, redstone wire, fire,
//! chorus plant, brewing stand) as the connection bits `blockstates::multipart_props` reads, and
//! the `shouldSideBeRendered` overrides those blocks carry.

use worldgen::blocks::id;
use worldgen::populate::behavior::except_with_piston;
use worldgen::populate::state as st;

use crate::blockstates::{DOWN, EAST, NORTH, SOUTH, UP, WEST, WIRE_SIDE, WIRE_UP};
use crate::models::Facing;
use crate::region::{props, Region};

/// The four horizontal sides in bit order (NORTH, EAST, SOUTH, WEST), with their bits.
const SIDES: [(st::Facing, u16); 4] = [(st::Facing::North, NORTH), (st::Facing::East, EAST), (st::Facing::South, SOUTH), (st::Facing::West, WEST)];

/// `Blocks.END_STONE`.
const END_STONE: u16 = 121;
const FENCES: [u16; 6] = [85, 188, 189, 190, 191, 192];
/// The oak, spruce, birch, jungle, dark oak and acacia fence gates.
const GATES: [u16; 6] = [107, 183, 184, 185, 186, 187];

fn is_fence(b: u16) -> bool {
    FENCES.contains(&b) || b == id::NETHER_BRICK_FENCE
}

fn is_pane(b: u16) -> bool {
    matches!(b, 101 | 102 | 160)
}

/// `BlockFenceGate.getBlockFaceShape` is `MIDDLE_POLE` when the gate's facing axis is the axis of
/// `face.rotateY()`: a gate facing east or west meets a neighbor on its north or south side.
fn gate_pole(n: u16, face: st::Facing) -> bool {
    if !GATES.contains(&(n >> 4)) {
        return false;
    }
    // FACING is the horizontal index of meta & 3: odd is west or east, even is south or north.
    let facing_on_x = n & 1 == 1;
    let face_on_z = matches!(face, st::Facing::North | st::Facing::South);
    facing_on_x == face_on_z
}

/// `BlockFence.isExcepBlockForAttachWithPiston` and `BlockWall.isExcepBlockForAttachWithPiston`.
fn fence_except(b: u16) -> bool {
    except_with_piston(b) || matches!(b, id::BARRIER | id::MELON_BLOCK | id::PUMPKIN | id::LIT_PUMPKIN)
}

/// `BlockPane.isExcepBlockForAttachWithPiston`.
fn pane_except(b: u16) -> bool {
    (id::WHITE_SHULKER_BOX..=id::BLACK_SHULKER_BOX).contains(&b)
        || matches!(
            b,
            id::LEAVES
                | id::LEAVES2
                | id::BEACON
                | id::CAULDRON
                | id::GLOWSTONE
                | id::ICE
                | id::SEA_LANTERN
                | id::PISTON
                | id::STICKY_PISTON
                | id::PISTON_HEAD
                | id::MELON_BLOCK
                | id::PUMPKIN
                | id::LIT_PUMPKIN
                | id::BARRIER
        )
}

/// `BlockFence.canConnectTo`: `face` is the side of the neighbor `n` that faces the fence.
fn fence_connects(fence: u16, n: u16, face: st::Facing) -> bool {
    let b = n >> 4;
    // MIDDLE_POLE with the fence's own material (wood, or the nether brick fence's rock), or a gate.
    let pole = (is_fence(b) && (fence == id::NETHER_BRICK_FENCE) == (b == id::NETHER_BRICK_FENCE)) || gate_pole(n, face);
    (!fence_except(b) && st::solid_face(n, face)) || pole
}

/// `BlockPane.attachesTo`.
fn pane_connects(n: u16, face: st::Facing) -> bool {
    let b = n >> 4;
    (!pane_except(b) && st::solid_face(n, face)) || is_pane(b)
}

/// `BlockWall.canConnectTo`.
fn wall_connects(n: u16, face: st::Facing) -> bool {
    let b = n >> 4;
    (!fence_except(b) && st::solid_face(n, face)) || b == id::COBBLESTONE_WALL || gate_pole(n, face)
}

/// `Blocks.FIRE.canCatchFire`: the blocks `BlockFire.init` gives a flammability above 0.
fn flammable(b: u16) -> bool {
    // planks, double and single wooden slab, the fence gates and fences, the wooden stairs, logs,
    // leaves, bookshelf, tnt, tall grass, double plant, flowers, dead bush, wool, vine, coal
    // block, hay block, carpet
    matches!(b, 5 | 125 | 126 | 53 | 134..=136 | 163 | 164 | 17 | 162 | 18 | 161 | 47 | 46 | 31 | 175 | 37 | 38 | 32 | 35 | 106 | 173 | 170 | 171)
        || GATES.contains(&b)
        || FENCES.contains(&b)
}

/// `Block.canProvidePower(state)` of the 1.12.2 blocks that return true.
fn can_provide_power(b: u16) -> bool {
    // redstone wire, torches, lever, buttons, pressure plates, tripwire hook, repeaters, comparators,
    // detector rail, daylight detectors, redstone block, observer, trapped chest
    matches!(b, 55 | 75 | 76 | 69 | 77 | 143 | 70 | 72 | 147 | 148 | 131 | 93 | 94 | 149 | 150 | 28 | 151 | 178 | 152 | 218 | 146)
}

/// `BlockRedstoneWire.canConnectTo(state, side)`; `side` is `null` for `canConnectUpwardsTo`.
fn wire_connects(n: u16, side: Option<st::Facing>) -> bool {
    match n >> 4 {
        55 => true,
        // isSameDiode: the repeater of either power state, along its facing axis.
        93 | 94 => side.is_some_and(|s| {
            let f = st::Facing::BY_HORIZONTAL[(n & 3) as usize];
            f == s || f.opposite() == s
        }),
        // BlockObserver.FACING = EnumFacing.getFront(meta & 7)
        218 => side == Some(st::Facing::VALUES[((n & 7) % 6) as usize]),
        b => can_provide_power(b) && side.is_some(),
    }
}

/// `BlockRedstoneWire.getAttachPosition`: `WIRE_UP`, `WIRE_SIDE` or 0 for NONE.
fn wire_attach(r: &Region, x: i32, y: i32, z: i32, dir: st::Facing) -> u16 {
    let (dx, _, dz) = dir.offset();
    let n = r.state(x + dx, y, z + dz);
    if !wire_connects(n, Some(dir)) && (st::is_normal_cube(n) || !wire_connects(r.state(x + dx, y - 1, z + dz), None)) {
        if !st::is_normal_cube(r.state(x, y + 1, z)) {
            let flag = st::is_top_solid(n) || n >> 4 == id::GLOWSTONE;
            if flag && wire_connects(r.state(x + dx, y + 1, z + dz), None) {
                return if st::is_block_normal_cube(n) { WIRE_UP } else { WIRE_SIDE };
            }
        }
        0
    } else {
        WIRE_SIDE
    }
}

/// `getActualState` of a multipart block as the connection bits for
/// `blockstates::multipart_props` (0 for any other block).
///
/// DROPPED: redstone wire's POWER (the file does not read it; the dust tint stays at power 0) and
/// fire's AGE (the file ignores it).
pub fn mask(r: &Region, state: u16, x: i32, y: i32, z: i32) -> u16 {
    let at = |f: st::Facing| {
        let (dx, dy, dz) = f.offset();
        r.state(x + dx, y + dy, z + dz)
    };
    let sides = |connects: &dyn Fn(u16, st::Facing) -> bool| {
        // The side of the neighbor that faces us is the opposite of the way we look.
        SIDES.iter().filter(|&&(f, _)| connects(at(f), f.opposite())).fold(0, |m, &(_, bit)| m | bit)
    };
    let block = state >> 4;
    match block {
        // BlockFence
        85 | 113 | 188..=192 => sides(&|n, face| fence_connects(block, n, face)),
        // BlockPane (glass pane, stained glass pane) and BlockPane's iron bars
        101 | 102 | 160 => sides(&pane_connects),
        // BlockWall: UP unless exactly two opposite sides connect, or something sits above.
        139 => {
            let m = sides(&wall_connects);
            let (n, e, s, w) = (m & NORTH != 0, m & EAST != 0, m & SOUTH != 0, m & WEST != 0);
            let straight = n && !e && s && !w || !n && e && !s && w;
            m | if !straight || at(st::Facing::Up) >> 4 != id::AIR { UP } else { 0 }
        }
        // BlockRedstoneWire: two bits a side, in NORTH, EAST, SOUTH, WEST order.
        55 => SIDES.iter().enumerate().fold(0, |m, (i, &(f, _))| m | wire_attach(r, x, y, z, f) << (2 * i)),
        // BlockFire: the sides and top only count over a floor that cannot hold or feed the fire.
        51 => {
            let below = at(st::Facing::Down);
            if st::is_top_solid(below) || flammable(below >> 4) {
                0
            } else {
                let m = sides(&|n, _| flammable(n >> 4));
                m | if flammable(at(st::Facing::Up) >> 4) { UP } else { 0 }
            }
        }
        // BlockChorusPlant: its own kind or a chorus flower, and end stone below.
        199 => {
            let join = |f: st::Facing| matches!(at(f) >> 4, 199 | 200);
            let m = sides(&|n, _| matches!(n >> 4, 199 | 200));
            m | if join(st::Facing::Up) { UP } else { 0 }
                | if join(st::Facing::Down) || at(st::Facing::Down) >> 4 == END_STONE { DOWN } else { 0 }
        }
        // BlockBrewingStand.getStateFromMeta: HAS_BOTTLE[i] is bit i of the meta.
        117 => state & 7,
        _ => 0,
    }
}

/// `Block.shouldSideBeRendered` for a multipart block, with the overrides of its class.
pub fn side_rendered(r: &Region, state: u16, mask: u16, x: i32, y: i32, z: i32, f: Facing) -> bool {
    let o = f.vec();
    let n = r.state(x + o[0], y + o[1], z + o[2]);
    let (block, nb) = (state >> 4, n >> 4);
    let default = || !props(n).opaque_cube;
    let side_bit = |f: Facing| match f {
        Facing::North => NORTH,
        Facing::East => EAST,
        Facing::South => SOUTH,
        Facing::West => WEST,
        _ => 0,
    };
    match block {
        // BlockFence
        85 | 113 | 188..=192 => true,
        // BlockWall
        139 => f != Facing::Down || default(),
        // BlockPane: never against its own kind; otherwise the bounding box, which reaches a
        // horizontal side only where the pane connects.
        101 | 102 | 160 => {
            if nb == block {
                return false;
            }
            let reaches = f.vec()[1] != 0 || mask & side_bit(f) != 0;
            !reaches || default()
        }
        // BlockChorusPlant
        199 => nb != 199 && nb != 200 && (f != Facing::Down || nb != END_STONE),
        // BlockRedstoneWire: REDSTONE_WIRE_AABB is 1/16 tall and spans a side only where it connects.
        55 => match f {
            Facing::Up => true,
            Facing::Down => default(),
            _ => !wire_reaches(mask, f) || default(),
        },
        // BlockBrewingStand: BASE_AABB is 0.125 tall.
        117 => f == Facing::Up || default(),
        // BlockFire: the full block box.
        _ => default(),
    }
}

/// Whether `REDSTONE_WIRE_AABB[getAABBIndex]` reaches the block edge on horizontal side `f`: the
/// wire connects that way, or it is the only line and runs the other way.
fn wire_reaches(mask: u16, f: Facing) -> bool {
    let c = |i: u32| (mask >> (2 * i)) & 3 != 0;
    let (n, e, s, w) = (c(0), c(1), c(2), c(3));
    match f {
        Facing::North => n || s && !n && !e && !w,
        Facing::South => s || n && !s && !e && !w,
        Facing::East => e || w && !e && !n && !s,
        Facing::West => w || e && !w && !n && !s,
        _ => false,
    }
}
