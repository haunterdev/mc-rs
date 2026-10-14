//! The per-block drop rules: `Block.getItemDropped`, `quantityDropped`,
//! `quantityDroppedWithBonus` and `damageDropped` with every 1.12.2 override. [`crate::drops`]
//! runs them in `dropBlockAsItemWithChance` order.

use worldgen::java_random::JavaRandom;

use crate::ids::{block, item};
use crate::registry::item_from_block;

#[inline]
pub(crate) fn block_of(state: u16) -> u16 {
    state >> 4
}

#[inline]
pub(crate) fn meta_of(state: u16) -> i32 {
    (state & 15) as i32
}

/// `EnumType.byMetadata(meta)`: anything out of range reads as the first constant.
#[inline]
pub(crate) fn by_meta(meta: i32, len: i32) -> i32 {
    if meta < 0 || meta >= len {
        0
    } else {
        meta
    }
}

pub(crate) fn is_ore(b: u16) -> bool {
    matches!(
        b,
        block::GOLD_ORE | block::IRON_ORE | block::COAL_ORE | block::LAPIS_ORE | block::DIAMOND_ORE | block::EMERALD_ORE | block::QUARTZ_ORE
    )
}

pub(crate) fn is_crops(b: u16) -> bool {
    matches!(b, block::WHEAT | block::CARROTS | block::POTATOES | block::BEETROOTS)
}

fn is_door(b: u16) -> bool {
    matches!(
        b,
        block::OAK_DOOR | block::IRON_DOOR | block::SPRUCE_DOOR | block::BIRCH_DOOR | block::JUNGLE_DOOR | block::ACACIA_DOOR | block::DARK_OAK_DOOR
    )
}

/// `BlockCrops.getMaxAge()`: 3 for `BlockBeetroot`, else 7.
pub(crate) fn crops_max_age(b: u16) -> i32 {
    if b == block::BEETROOTS {
        3
    } else {
        7
    }
}

/// `BlockCrops.getSeed()` and `getCrop()`
pub(crate) fn crops_seed_and_crop(b: u16) -> (u16, u16) {
    match b {
        block::CARROTS => (item::CARROT, item::CARROT),
        block::POTATOES => (item::POTATO, item::POTATO),
        block::BEETROOTS => (item::BEETROOT_SEEDS, item::BEETROOT),
        _ => (item::WHEAT_SEEDS, item::WHEAT),
    }
}

/// `BlockDoor.getItem()`
fn door_item(b: u16) -> u16 {
    match b {
        block::IRON_DOOR => item::IRON_DOOR,
        block::SPRUCE_DOOR => item::SPRUCE_DOOR,
        block::BIRCH_DOOR => item::BIRCH_DOOR,
        block::JUNGLE_DOOR => item::JUNGLE_DOOR,
        block::ACACIA_DOOR => item::ACACIA_DOOR,
        block::DARK_OAK_DOOR => item::DARK_OAK_DOOR,
        _ => item::OAK_DOOR,
    }
}

/// `BlockDoublePlant.VARIANT` of a lower half: `EnumPlantType.byMetadata(meta & 7)`.
pub(crate) fn double_plant_variant(meta: i32) -> i32 {
    by_meta(meta & 7, 6)
}

/// `BlockDoublePlant.EnumPlantType` GRASS and FERN.
pub(crate) const DOUBLE_GRASS: i32 = 2;
pub(crate) const DOUBLE_FERN: i32 = 3;

/// `Block.getItemDropped(state, rand, fortune)` with every override; draws where Java draws
/// (tall grass, double grass, gravel).
pub fn get_item_dropped(state: u16, rand: &mut JavaRandom, fortune: i32) -> u16 {
    let b = block_of(state);
    let meta = meta_of(state);
    match b {
        // BlockStone: STONE drops cobblestone, the rest themselves
        block::STONE => {
            if by_meta(meta, 7) == 0 {
                item_from_block(block::COBBLESTONE)
            } else {
                item_from_block(block::STONE)
            }
        }
        // BlockGrass, BlockMycelium, BlockGrassPath, BlockFarmland: Blocks.DIRT.getItemDropped
        block::GRASS | block::MYCELIUM | block::GRASS_PATH | block::FARMLAND => item_from_block(block::DIRT),
        // BlockGravel
        block::GRAVEL => {
            let f = fortune.min(3);
            if rand.next_int_bounded(10 - f * 3) == 0 {
                item::FLINT
            } else {
                item_from_block(block::GRAVEL)
            }
        }
        // BlockOre
        block::COAL_ORE => item::COAL,
        block::DIAMOND_ORE => item::DIAMOND,
        block::LAPIS_ORE => item::DYE,
        block::EMERALD_ORE => item::EMERALD,
        block::QUARTZ_ORE => item::QUARTZ,
        // BlockLeaves
        block::LEAVES | block::LEAVES2 => item_from_block(block::SAPLING),
        // BlockBed: the foot drops nothing
        block::BED => {
            if meta & 8 == 0 {
                item::AIR
            } else {
                item::BED
            }
        }
        // BlockWeb, BlockTripWire
        block::WEB | block::TRIPWIRE => item::STRING,
        // BlockTallGrass
        block::TALLGRASS => {
            if rand.next_int_bounded(8) == 0 {
                item::WHEAT_SEEDS
            } else {
                item::AIR
            }
        }
        // BlockDeadBush
        block::DEADBUSH => item::STICK,
        // BlockDoublePlant: nothing from the upper half or a fern, seeds one time in 8 from
        // double grass, the plant itself otherwise
        block::DOUBLE_PLANT => {
            if meta & 8 != 0 {
                item::AIR
            } else {
                match double_plant_variant(meta) {
                    DOUBLE_FERN => item::AIR,
                    DOUBLE_GRASS => {
                        if rand.next_int_bounded(8) == 0 {
                            item::WHEAT_SEEDS
                        } else {
                            item::AIR
                        }
                    }
                    _ => item_from_block(block::DOUBLE_PLANT),
                }
            }
        }
        // BlockStoneSlab, BlockStoneSlabNew, BlockWoodSlab, BlockPurpurSlab: the half slab
        block::STONE_SLAB | block::DOUBLE_STONE_SLAB => item_from_block(block::STONE_SLAB),
        block::STONE_SLAB2 | block::DOUBLE_STONE_SLAB2 => item_from_block(block::STONE_SLAB2),
        block::WOODEN_SLAB | block::DOUBLE_WOODEN_SLAB => item_from_block(block::WOODEN_SLAB),
        block::PURPUR_SLAB | block::PURPUR_DOUBLE_SLAB => item_from_block(block::PURPUR_SLAB),
        // BlockBookshelf
        block::BOOKSHELF => item::BOOK,
        // BlockRedstoneWire
        block::REDSTONE_WIRE => item::REDSTONE,
        // BlockCrops: the crop when fully grown, else the seed
        b if is_crops(b) => {
            let (seed, crop) = crops_seed_and_crop(b);
            if meta >= crops_max_age(b) {
                crop
            } else {
                seed
            }
        }
        // BlockFurnace
        block::FURNACE | block::LIT_FURNACE => item_from_block(block::FURNACE),
        // BlockSign
        block::STANDING_SIGN | block::WALL_SIGN => item::SIGN,
        // BlockDoor: the upper half drops nothing
        b if is_door(b) => {
            if meta & 8 != 0 {
                item::AIR
            } else {
                door_item(b)
            }
        }
        // BlockRedstoneOre
        block::REDSTONE_ORE | block::LIT_REDSTONE_ORE => item::REDSTONE,
        // BlockRedstoneTorch
        block::UNLIT_REDSTONE_TORCH | block::REDSTONE_TORCH => item_from_block(block::REDSTONE_TORCH),
        // BlockSnow, BlockSnowBlock
        block::SNOW_LAYER | block::SNOW => item::SNOWBALL,
        // BlockClay
        block::CLAY => item::CLAY_BALL,
        // BlockReed
        block::REEDS => item::REEDS,
        // BlockGlowstone
        block::GLOWSTONE => item::GLOWSTONE_DUST,
        // BlockCake
        block::CAKE => item::AIR,
        // BlockRedstoneRepeater, BlockRedstoneComparator
        block::UNPOWERED_REPEATER | block::POWERED_REPEATER => item::REPEATER,
        block::UNPOWERED_COMPARATOR | block::POWERED_COMPARATOR => item::COMPARATOR,
        // BlockHugeMushroom: the small mushroom
        block::BROWN_MUSHROOM_BLOCK => item_from_block(block::BROWN_MUSHROOM),
        block::RED_MUSHROOM_BLOCK => item_from_block(block::RED_MUSHROOM),
        // BlockPane: canDrop is false for glass panes (BlockStainedGlassPane too)
        block::GLASS_PANE | block::STAINED_GLASS_PANE => item::AIR,
        // BlockStem, BlockVine, BlockNetherWart, BlockEndPortalFrame, BlockChorusFlower,
        // BlockLiquid, BlockMobSpawner, BlockPistonMoving, BlockEmptyDrops (bedrock)
        block::PUMPKIN_STEM
        | block::MELON_STEM
        | block::VINE
        | block::NETHER_WART
        | block::END_PORTAL_FRAME
        | block::CHORUS_FLOWER
        | block::FLOWING_WATER
        | block::WATER
        | block::FLOWING_LAVA
        | block::LAVA
        | block::MOB_SPAWNER
        | block::PISTON_EXTENSION
        | block::BEDROCK => item::AIR,
        // BlockBrewingStand, BlockCauldron, BlockFlowerPot, BlockSkull, BlockBanner
        block::BREWING_STAND => item::BREWING_STAND,
        block::CAULDRON => item::CAULDRON,
        block::FLOWER_POT => item::FLOWER_POT,
        block::SKULL => item::SKULL,
        block::STANDING_BANNER | block::WALL_BANNER => item::BANNER,
        // BlockDaylightDetector
        block::DAYLIGHT_DETECTOR | block::DAYLIGHT_DETECTOR_INVERTED => item_from_block(block::DAYLIGHT_DETECTOR),
        // BlockRedstoneLight
        block::REDSTONE_LAMP | block::LIT_REDSTONE_LAMP => item_from_block(block::REDSTONE_LAMP),
        // BlockObsidian, BlockEnderChest
        block::OBSIDIAN | block::ENDER_CHEST => item_from_block(block::OBSIDIAN),
        // BlockSeaLantern
        block::SEA_LANTERN => item::PRISMARINE_CRYSTALS,
        // BlockMelon
        block::MELON_BLOCK => item::MELON,
        // BlockChorusPlant
        block::CHORUS_PLANT => item::CHORUS_FRUIT,
        // Block: Item.getItemFromBlock(this), which is Items.AIR for blocks without an item
        _ => item_from_block(b),
    }
}

/// `Block.quantityDropped(random)` with every override.
pub fn quantity_dropped(state: u16, rand: &mut JavaRandom) -> i32 {
    let b = block_of(state);
    match b {
        block::BOOKSHELF => 3,
        // BlockChorusPlant
        block::CHORUS_PLANT => rand.next_int_bounded(2),
        block::CLAY | block::SNOW => 4,
        // BlockDeadBush
        block::DEADBUSH => rand.next_int_bounded(3),
        block::ENDER_CHEST => 8,
        // BlockGlowstone
        block::GLOWSTONE => 2 + rand.next_int_bounded(3),
        // BlockHugeMushroom
        block::BROWN_MUSHROOM_BLOCK | block::RED_MUSHROOM_BLOCK => (rand.next_int_bounded(10) - 7).max(0),
        // BlockLeaves
        block::LEAVES | block::LEAVES2 => i32::from(rand.next_int_bounded(20) == 0),
        // BlockMelon
        block::MELON_BLOCK => 3 + rand.next_int_bounded(5),
        // BlockOre: lapis 4 to 8
        block::LAPIS_ORE => 4 + rand.next_int_bounded(5),
        // BlockRedstoneOre
        block::REDSTONE_ORE | block::LIT_REDSTONE_ORE => 4 + rand.next_int_bounded(2),
        // BlockSeaLantern
        block::SEA_LANTERN => 2 + rand.next_int_bounded(2),
        // BlockSlab: a double slab drops two halves
        block::DOUBLE_STONE_SLAB | block::DOUBLE_WOODEN_SLAB | block::DOUBLE_STONE_SLAB2 | block::PURPUR_DOUBLE_SLAB => 2,
        // BlockCake, BlockCommandBlock, BlockEmptyDrops, BlockEndGateway, BlockEndPortal, BlockFire,
        // BlockGlass, BlockIce (and BlockFrostedIce), BlockLiquid, BlockMobSpawner,
        // BlockNetherWart, BlockPackedIce, BlockPistonExtension, BlockPortal, BlockSilverfish,
        // BlockSnow, BlockStainedGlass, BlockStructure, BlockVine
        block::CAKE
        | block::COMMAND_BLOCK
        | block::REPEATING_COMMAND_BLOCK
        | block::CHAIN_COMMAND_BLOCK
        | block::BEDROCK
        | block::END_GATEWAY
        | block::END_PORTAL
        | block::FIRE
        | block::GLASS
        | block::ICE
        | block::FROSTED_ICE
        | block::FLOWING_WATER
        | block::WATER
        | block::FLOWING_LAVA
        | block::LAVA
        | block::MOB_SPAWNER
        | block::NETHER_WART
        | block::PACKED_ICE
        | block::PISTON_HEAD
        | block::PORTAL
        | block::MONSTER_EGG
        | block::SNOW_LAYER
        | block::STAINED_GLASS
        | block::STRUCTURE_BLOCK
        | block::VINE => 0,
        _ => 1,
    }
}

/// `Block.quantityDroppedWithBonus(fortune, random)` with every override.
pub fn quantity_dropped_with_bonus(state: u16, fortune: i32, rand: &mut JavaRandom) -> i32 {
    let b = block_of(state);
    match b {
        // BlockGlowstone
        block::GLOWSTONE => (quantity_dropped(state, rand) + rand.next_int_bounded(fortune + 1)).clamp(1, 4),
        // BlockMelon
        block::MELON_BLOCK => (quantity_dropped(state, rand) + rand.next_int_bounded(1 + fortune)).min(9),
        // BlockOre
        b if is_ore(b) => {
            // getItemDropped(the first valid state, ...) draws nothing for ores.
            if fortune > 0 && item_from_block(b) != get_item_dropped(b << 4, rand, fortune) {
                let i = (rand.next_int_bounded(fortune + 2) - 1).max(0);
                quantity_dropped(state, rand) * (i + 1)
            } else {
                quantity_dropped(state, rand)
            }
        }
        // BlockRedstoneOre
        block::REDSTONE_ORE | block::LIT_REDSTONE_ORE => quantity_dropped(state, rand) + rand.next_int_bounded(fortune + 1),
        // BlockSeaLantern
        block::SEA_LANTERN => (quantity_dropped(state, rand) + rand.next_int_bounded(fortune + 1)).clamp(1, 5),
        // BlockTallGrass
        block::TALLGRASS => 1 + rand.next_int_bounded(fortune * 2 + 1),
        _ => quantity_dropped(state, rand),
    }
}

/// `Block.damageDropped(state)` with every override: the metadata of the dropped item.
pub fn damage_dropped(state: u16) -> i32 {
    let b = block_of(state);
    let meta = meta_of(state);
    match b {
        // BlockAnvil: DAMAGE
        block::ANVIL => (meta & 15) >> 2,
        // BlockColored (wool, concrete, stained clay), BlockCarpet, BlockConcretePowder,
        // BlockStainedGlass, BlockStainedGlassPane: COLOR
        block::WOOL
        | block::CONCRETE
        | block::STAINED_HARDENED_CLAY
        | block::CARPET
        | block::CONCRETE_POWDER
        | block::STAINED_GLASS
        | block::STAINED_GLASS_PANE => meta,
        // BlockDirt: podzol drops dirt
        block::DIRT => {
            let v = by_meta(meta, 3);
            if v == 2 {
                0
            } else {
                v
            }
        }
        // BlockDoublePlant: the lower half's variant, except double grass
        block::DOUBLE_PLANT => {
            if meta & 8 == 0 && double_plant_variant(meta) != DOUBLE_GRASS {
                double_plant_variant(meta)
            } else {
                0
            }
        }
        // BlockFlower: EnumFlowerType.getMeta() via getType(color, meta)
        block::YELLOW_FLOWER => 0,
        block::RED_FLOWER => by_meta(meta, 9),
        // BlockOldLeaf, BlockOldLog: VARIANT (meta & 3)
        block::LEAVES | block::LOG => meta & 3,
        // BlockNewLeaf: VARIANT ((meta & 3) + 4), acacia 4 and dark oak 5
        block::LEAVES2 => by_meta((meta & 3) + 4, 6),
        // BlockNewLog: VARIANT - 4
        block::LOG2 => by_meta((meta & 3) + 4, 6) - 4,
        // BlockOre: lapis drops blue dye (dye damage 4)
        block::LAPIS_ORE => 4,
        // BlockPlanks, BlockPrismarine, BlockRedSandstone, BlockSand, BlockSandStone, BlockStone,
        // BlockStoneBrick, BlockWall: VARIANT/TYPE.getMetadata()
        block::PLANKS => by_meta(meta, 6),
        block::PRISMARINE | block::RED_SANDSTONE | block::SANDSTONE => by_meta(meta, 3),
        block::SAND => by_meta(meta, 2),
        block::STONE => by_meta(meta, 7),
        block::STONEBRICK => by_meta(meta, 4),
        block::COBBLESTONE_WALL => by_meta(meta, 2),
        // BlockQuartz: the x and z pillars drop the y pillar
        block::QUARTZ_BLOCK => match meta {
            3 | 4 => 2,
            m => by_meta(m, 5),
        },
        // BlockSapling: TYPE (meta & 7)
        block::SAPLING => by_meta(meta & 7, 6),
        // BlockSponge: WET
        block::SPONGE => meta & 1,
        // BlockStoneSlab, BlockStoneSlabNew, BlockWoodSlab: VARIANT (meta & 7)
        block::STONE_SLAB | block::DOUBLE_STONE_SLAB => meta & 7,
        block::STONE_SLAB2 | block::DOUBLE_STONE_SLAB2 => by_meta(meta & 7, 1),
        block::WOODEN_SLAB | block::DOUBLE_WOODEN_SLAB => by_meta(meta & 7, 6),
        _ => 0,
    }
}
