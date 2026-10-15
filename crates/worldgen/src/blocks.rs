//! Block states as `Block.BLOCK_STATE_IDS` numbers them: `block id << 4 | metadata`.
//! `ChunkPrimer` stores these as `char`, so they are `u16` here.

/// Builds a state id from a block id and its metadata.
pub const fn state(id: u16, meta: u16) -> u16 {
    id << 4 | meta
}

/// Block id of a state.
#[inline(always)]
pub const fn block_of(state: u16) -> u16 {
    state >> 4
}

/// The seven `BlockDoor`s: oak, iron, and spruce to dark oak.
pub const fn is_door(block: u16) -> bool {
    matches!(block, id::WOODEN_DOOR | id::IRON_DOOR) || (block >= id::SPRUCE_DOOR && block <= id::DARK_OAK_DOOR)
}

pub mod id {
    pub const AIR: u16 = 0;
    pub const STONE: u16 = 1;
    pub const GRASS: u16 = 2;
    pub const DIRT: u16 = 3;
    pub const COBBLESTONE: u16 = 4;
    pub const PLANKS: u16 = 5;
    pub const SAPLING: u16 = 6;
    pub const BEDROCK: u16 = 7;
    pub const FLOWING_WATER: u16 = 8;
    pub const WATER: u16 = 9;
    pub const FLOWING_LAVA: u16 = 10;
    pub const LAVA: u16 = 11;
    pub const SAND: u16 = 12;
    pub const GRAVEL: u16 = 13;
    pub const GOLD_ORE: u16 = 14;
    pub const IRON_ORE: u16 = 15;
    pub const COAL_ORE: u16 = 16;
    pub const LOG: u16 = 17;
    pub const LEAVES: u16 = 18;
    pub const GLASS: u16 = 20;
    pub const LAPIS_ORE: u16 = 21;
    pub const SANDSTONE: u16 = 24;
    pub const STICKY_PISTON: u16 = 29;
    pub const TALLGRASS: u16 = 31;
    pub const DEADBUSH: u16 = 32;
    pub const PISTON: u16 = 33;
    pub const PISTON_HEAD: u16 = 34;
    pub const YELLOW_FLOWER: u16 = 37;
    pub const RED_FLOWER: u16 = 38;
    pub const BROWN_MUSHROOM: u16 = 39;
    pub const RED_MUSHROOM: u16 = 40;
    pub const DOUBLE_STONE_SLAB: u16 = 43;
    pub const STONE_SLAB: u16 = 44;
    pub const BRICK_BLOCK: u16 = 45;
    pub const MOSSY_COBBLESTONE: u16 = 48;
    pub const OBSIDIAN: u16 = 49;
    pub const TORCH: u16 = 50;
    pub const FIRE: u16 = 51;
    pub const MOB_SPAWNER: u16 = 52;
    pub const CHEST: u16 = 54;
    pub const DIAMOND_ORE: u16 = 56;
    pub const WHEAT: u16 = 59;
    pub const FARMLAND: u16 = 60;
    pub const STANDING_SIGN: u16 = 63;
    pub const WOODEN_DOOR: u16 = 64;
    pub const LADDER: u16 = 65;
    pub const WALL_SIGN: u16 = 68;
    pub const IRON_DOOR: u16 = 71;
    pub const REDSTONE_ORE: u16 = 73;
    pub const SNOW_LAYER: u16 = 78;
    pub const ICE: u16 = 79;
    pub const SNOW: u16 = 80;
    pub const CACTUS: u16 = 81;
    pub const CLAY: u16 = 82;
    pub const REEDS: u16 = 83;
    pub const OAK_FENCE: u16 = 85;
    pub const PUMPKIN: u16 = 86;
    pub const GLOWSTONE: u16 = 89;
    pub const PORTAL: u16 = 90;
    pub const LIT_PUMPKIN: u16 = 91;
    pub const STAINED_GLASS: u16 = 95;
    pub const TRAPDOOR: u16 = 96;
    pub const MONSTER_EGG: u16 = 97;
    pub const STONEBRICK: u16 = 98;
    pub const BROWN_MUSHROOM_BLOCK: u16 = 99;
    pub const RED_MUSHROOM_BLOCK: u16 = 100;
    pub const MELON_BLOCK: u16 = 103;
    pub const PUMPKIN_STEM: u16 = 104;
    pub const MELON_STEM: u16 = 105;
    pub const VINE: u16 = 106;
    pub const MYCELIUM: u16 = 110;
    pub const WATERLILY: u16 = 111;
    pub const NETHER_BRICK_FENCE: u16 = 113;
    pub const CAULDRON: u16 = 118;
    pub const DOUBLE_WOODEN_SLAB: u16 = 125;
    pub const WOODEN_SLAB: u16 = 126;
    pub const COCOA: u16 = 127;
    pub const EMERALD_ORE: u16 = 129;
    pub const BEACON: u16 = 138;
    pub const COBBLESTONE_WALL: u16 = 139;
    pub const CARROTS: u16 = 141;
    pub const POTATOES: u16 = 142;
    pub const STAINED_HARDENED_CLAY: u16 = 159;
    pub const LEAVES2: u16 = 161;
    pub const LOG2: u16 = 162;
    pub const BARRIER: u16 = 166;
    pub const IRON_TRAPDOOR: u16 = 167;
    pub const SEA_LANTERN: u16 = 169;
    pub const HARDENED_CLAY: u16 = 172;
    pub const PACKED_ICE: u16 = 174;
    pub const DOUBLE_PLANT: u16 = 175;
    pub const RED_SANDSTONE: u16 = 179;
    pub const DOUBLE_STONE_SLAB2: u16 = 181;
    pub const STONE_SLAB2: u16 = 182;
    pub const SPRUCE_FENCE: u16 = 188;
    pub const ACACIA_FENCE: u16 = 192;
    pub const SPRUCE_DOOR: u16 = 193;
    pub const BIRCH_DOOR: u16 = 194;
    pub const JUNGLE_DOOR: u16 = 195;
    pub const ACACIA_DOOR: u16 = 196;
    pub const DARK_OAK_DOOR: u16 = 197;
    pub const PURPUR_DOUBLE_SLAB: u16 = 204;
    pub const PURPUR_SLAB: u16 = 205;
    pub const BEETROOTS: u16 = 207;
    pub const END_GATEWAY: u16 = 209;
    pub const FROSTED_ICE: u16 = 212;
    pub const STRUCTURE_VOID: u16 = 217;
    pub const WHITE_SHULKER_BOX: u16 = 219;
    pub const BLACK_SHULKER_BOX: u16 = 234;
}

pub const AIR: u16 = state(id::AIR, 0);
pub const STONE: u16 = state(id::STONE, 0);
pub const GRASS: u16 = state(id::GRASS, 0);
pub const DIRT: u16 = state(id::DIRT, 0);
/// `BlockDirt.DirtType.COARSE_DIRT`
pub const COARSE_DIRT: u16 = state(id::DIRT, 1);
/// `BlockDirt.DirtType.PODZOL`
pub const PODZOL: u16 = state(id::DIRT, 2);
pub const BEDROCK: u16 = state(id::BEDROCK, 0);
pub const FLOWING_WATER: u16 = state(id::FLOWING_WATER, 0);
pub const WATER: u16 = state(id::WATER, 0);
pub const FLOWING_LAVA: u16 = state(id::FLOWING_LAVA, 0);
pub const LAVA: u16 = state(id::LAVA, 0);
pub const SAND: u16 = state(id::SAND, 0);
/// `BlockSand.EnumType.RED_SAND`
pub const RED_SAND: u16 = state(id::SAND, 1);
pub const GRAVEL: u16 = state(id::GRAVEL, 0);
pub const SANDSTONE: u16 = state(id::SANDSTONE, 0);
pub const RED_SANDSTONE: u16 = state(id::RED_SANDSTONE, 0);
pub const ICE: u16 = state(id::ICE, 0);
pub const SNOW_LAYER: u16 = state(id::SNOW_LAYER, 0);
pub const SNOW: u16 = state(id::SNOW, 0);
pub const MYCELIUM: u16 = state(id::MYCELIUM, 0);
pub const WATERLILY: u16 = state(id::WATERLILY, 0);
pub const HARDENED_CLAY: u16 = state(id::HARDENED_CLAY, 0);

/// `EnumDyeColor` metadata, as used by `BlockColored.COLOR`.
pub mod dye {
    pub const WHITE: u16 = 0;
    pub const ORANGE: u16 = 1;
    pub const MAGENTA: u16 = 2;
    pub const LIGHT_BLUE: u16 = 3;
    pub const YELLOW: u16 = 4;
    pub const LIME: u16 = 5;
    pub const PINK: u16 = 6;
    pub const GRAY: u16 = 7;
    pub const SILVER: u16 = 8;
    pub const CYAN: u16 = 9;
    pub const PURPLE: u16 = 10;
    pub const BLUE: u16 = 11;
    pub const BROWN: u16 = 12;
    pub const GREEN: u16 = 13;
    pub const RED: u16 = 14;
    pub const BLACK: u16 = 15;
    /// `EnumDyeColor.getName()` in metadata order.
    pub const NAMES: [&str; 16] = [
        "white", "orange", "magenta", "light_blue", "yellow", "lime", "pink", "gray", "silver", "cyan", "purple",
        "blue", "brown", "green", "red", "black",
    ];
}

/// `Blocks.STAINED_HARDENED_CLAY` with the given `EnumDyeColor` metadata.
pub const fn stained_hardened_clay(color: u16) -> u16 {
    state(id::STAINED_HARDENED_CLAY, color)
}

/// `getMaterial() == Material.AIR` for anything a ChunkPrimer can hold during terrain generation.
#[inline(always)]
pub const fn is_air(state: u16) -> bool {
    block_of(state) == id::AIR
}

/// `getMaterial() == Material.WATER`: still and flowing water.
#[inline(always)]
pub const fn is_water(state: u16) -> bool {
    let b = block_of(state);
    b == id::WATER || b == id::FLOWING_WATER
}

/// `block.getDefaultState()` for the blocks generation can place.
pub const fn default_state(state: u16) -> u16 {
    block_of(state) << 4
}
