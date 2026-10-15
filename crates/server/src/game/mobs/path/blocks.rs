//! The block questions the path code asks of `IBlockState`: `Block.isPassable`, the material,
//! door and gate state, and `getBoundingBox().maxY`. Block ids are 1.12.2's numeric ids
//! (`Block.registerBlocks`).

use sim::collision::BlockAccess;
use sim::raytrace;
use sim::region::{props, Mat};
use worldgen::blocks::{id, is_door};

const RAILS: [u16; 4] = [27, 28, 66, 157];
const FENCE_GATES: [u16; 6] = [107, 183, 184, 185, 186, 187];
const FENCES: [u16; 7] = [id::OAK_FENCE, id::NETHER_BRICK_FENCE, 188, 189, 190, 191, 192];
const PRESSURE_PLATES: [u16; 4] = [70, 72, 147, 148];
const BANNERS: [u16; 2] = [176, 177];
const CHORUS_PLANT: u16 = 199;
pub const MAGMA: u16 = 213;
/// `Blocks.FIRE`
pub const FIRE: u16 = id::FIRE;

pub fn material(state: u16) -> Mat {
    props(state).mat
}

/// `World.canSeeSky(pos)`: no block that stops light (`getLightOpacity() > 0`) at or above `y`,
/// which is `y >= heightMap`. A chunk that is not loaded reads as open.
pub fn can_see_sky(w: &impl BlockAccess, x: i32, y: i32, z: i32) -> bool {
    (y.max(0)..256).all(|yy| props(w.state(x, yy, z)).opacity == 0)
}

pub fn is_air(state: u16) -> bool {
    material(state) == Mat::Air
}

/// `Material.isSolid`
pub fn is_solid(state: u16) -> bool {
    props(state).solid_material()
}

pub fn is_rail(state: u16) -> bool {
    RAILS.contains(&(state >> 4))
}

pub fn is_fence_gate(state: u16) -> bool {
    FENCE_GATES.contains(&(state >> 4))
}

/// `BlockFence` or `BlockWall`.
pub fn is_fence_or_wall(state: u16) -> bool {
    FENCES.contains(&(state >> 4)) || state >> 4 == id::COBBLESTONE_WALL
}

pub fn is_trapdoor(state: u16) -> bool {
    matches!(state >> 4, id::TRAPDOOR | id::IRON_TRAPDOOR)
}

/// `Material.IRON` doors (`BlockDoor` of the iron door); the others are wood.
pub fn is_iron_door(state: u16) -> bool {
    state >> 4 == id::IRON_DOOR
}

/// `BlockDoor.OPEN` of the state, read off the lower half (`getActualState`).
pub fn door_open(w: &impl BlockAccess, state: u16, x: i32, y: i32, z: i32) -> bool {
    sim::door::actual(|x, y, z| w.state(x, y, z), state, x, y, z).0 & 4 != 0
}

/// `Block.isPassable(world, pos)` and its overrides.
pub fn is_passable(w: &impl BlockAccess, x: i32, y: i32, z: i32) -> bool {
    let s = w.state(x, y, z);
    let b = s >> 4;
    if is_door(b) {
        // BlockDoor.isPassable: isOpen(combineMetadata)
        return door_open(w, s, x, y, z);
    }
    if is_fence_gate(s) || is_trapdoor(s) {
        // BlockFenceGate: OPEN; BlockTrapDoor: !OPEN. Both keep OPEN in meta bit 2.
        let open = s & 4 != 0;
        return if is_trapdoor(s) { !open } else { open };
    }
    if is_fence_or_wall(s) || b == CHORUS_PLANT {
        return false;
    }
    if b == id::SNOW_LAYER {
        // BlockSnow.isPassable: LAYERS < 5, LAYERS = meta + 1.
        return (s & 7) + 1 < 5;
    }
    if BANNERS.contains(&b) || b == id::STANDING_SIGN || b == id::WALL_SIGN || b == id::CAULDRON || PRESSURE_PLATES.contains(&b) {
        return true;
    }
    match material(s) {
        // BlockLiquid.isPassable: material != LAVA
        Mat::Water => true,
        Mat::Lava => false,
        // Block.isPassable: !material.blocksMovement()
        _ => !is_solid(s),
    }
}

/// `iblockstate.getBoundingBox(world, pos).maxY`; air has `NULL_AABB`.
pub fn bounding_max_y(w: &impl BlockAccess, x: i32, y: i32, z: i32) -> f64 {
    let s = w.state(x, y, z);
    if is_air(s) {
        return 0.0;
    }
    raytrace::bounding_box(w, s, (x, y, z)).max_y
}
