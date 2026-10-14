//! `BlockModelShapes`' state mappers for the blocks the generator and population place: the
//! blockstate file and variant key a state renders with.

use worldgen::blocks::{dye, id};

/// `BlockOldLog.EnumType` / `BlockPlanks.EnumType` names, by meta.
pub(crate) const WOODS: [&str; 6] = ["oak", "spruce", "birch", "jungle", "acacia", "dark_oak"];
/// `BlockStoneSlab.EnumType.getName` by VARIANT (meta & 7).
pub(crate) const STONE_SLABS: [&str; 8] =
    ["stone", "sandstone", "wood_old", "cobblestone", "brick", "stone_brick", "nether_brick", "quartz"];

/// The double block a half slab joins into (`ItemSlab`'s `doubleSlab`), or `None` for a block that
/// is not a half slab.
pub fn double_slab(block: u16) -> Option<u16> {
    Some(match block {
        id::STONE_SLAB => id::DOUBLE_STONE_SLAB,
        id::WOODEN_SLAB => id::DOUBLE_WOODEN_SLAB,
        id::STONE_SLAB2 => id::DOUBLE_STONE_SLAB2,
        id::PURPUR_SLAB => id::PURPUR_DOUBLE_SLAB,
        _ => return None,
    })
}

/// The half slab a double slab drops and picks (`getItem`), or `None` for any other block.
pub fn half_slab(block: u16) -> Option<u16> {
    Some(match block {
        id::DOUBLE_STONE_SLAB => id::STONE_SLAB,
        id::DOUBLE_WOODEN_SLAB => id::WOODEN_SLAB,
        id::DOUBLE_STONE_SLAB2 => id::STONE_SLAB2,
        id::PURPUR_DOUBLE_SLAB => id::PURPUR_SLAB,
        _ => return None,
    })
}

/// `BlockHugeMushroom.EnumType` by meta; 11 to 13 are unused.
const MUSHROOM_BLOCK: [&str; 16] = [
    "all_inside",
    "north_west",
    "north",
    "north_east",
    "west",
    "center",
    "east",
    "south_west",
    "south",
    "south_east",
    "stem",
    "",
    "",
    "",
    "all_outside",
    "all_stem",
];

/// `BlockDoublePlant.EnumPlantType` by meta.
pub(crate) const DOUBLE_PLANTS: [&str; 6] = ["sunflower", "syringa", "double_grass", "double_fern", "double_rose", "paeonia"];

/// `BlockFlower.EnumFlowerType` of the red flower, by meta.
pub(crate) const RED_FLOWERS: [&str; 9] =
    ["poppy", "blue_orchid", "allium", "houstonia", "red_tulip", "orange_tulip", "white_tulip", "pink_tulip", "oxeye_daisy"];

/// `EnumFacing.byHorizontalIndex` names.
const BY_HORIZONTAL: [&str; 4] = ["south", "west", "north", "east"];

/// The blockstate file and variant key `state` renders with. `flag` is the property
/// `getActualState` adds: SNOWY for grass, mycelium and podzol, UP for vines. A double plant's
/// upper half arrives as `175:(8 | variant)`, the variant `getActualState` copies from below.
pub fn blockstate_of(s: u16, flag: bool) -> Option<(String, String)> {
    let (block, meta) = (s >> 4, (s & 15) as usize);
    let snow = format!("snowy={flag}");
    let normal = |f: &str| (f.to_string(), "normal".to_string());
    Some(match block {
        id::STONE => {
            const STONES: [&str; 7] =
                ["stone", "granite", "smooth_granite", "diorite", "smooth_diorite", "andesite", "smooth_andesite"];
            normal(STONES.get(meta)?)
        }
        id::GRASS => ("grass".into(), snow),
        // BlockDirt's state mapper names the file after the variant and drops SNOWY except for podzol.
        id::DIRT => match meta {
            0 => normal("dirt"),
            1 => normal("coarse_dirt"),
            2 => ("podzol".into(), snow),
            _ => return None,
        },
        id::BEDROCK => normal("bedrock"),
        id::SAND => normal(if meta == 1 { "red_sand" } else { "sand" }),
        id::GRAVEL => normal("gravel"),
        id::SANDSTONE if meta == 0 => normal("sandstone"),
        id::RED_SANDSTONE if meta == 0 => normal("red_sandstone"),
        id::ICE => normal("ice"),
        id::SNOW => normal("snow"),
        id::MYCELIUM => ("mycelium".into(), snow),
        id::WATERLILY => normal("waterlily"),
        id::HARDENED_CLAY => normal("hardened_clay"),
        id::STAINED_HARDENED_CLAY => (format!("{}_stained_hardened_clay", dye::NAMES[meta]), "normal".into()),
        id::COBBLESTONE => normal("cobblestone"),
        // BlockPlanks: withName(VARIANT).withSuffix("_planks").
        id::PLANKS => normal(&format!("{}_planks", WOODS.get(meta)?)),
        id::GLASS => normal("glass"),
        id::BRICK_BLOCK => normal("brick_block"),
        // BlockStoneBrick: withName(VARIANT).
        id::STONEBRICK => {
            normal(["stonebrick", "mossy_stonebrick", "cracked_stonebrick", "chiseled_stonebrick"].get(meta)?)
        }
        id::MOSSY_COBBLESTONE => normal("mossy_cobblestone"),
        id::OBSIDIAN => normal("obsidian"),
        id::MOB_SPAWNER => normal("mob_spawner"),
        id::COAL_ORE => normal("coal_ore"),
        id::IRON_ORE => normal("iron_ore"),
        id::GOLD_ORE => normal("gold_ore"),
        id::LAPIS_ORE => normal("lapis_ore"),
        id::DIAMOND_ORE => normal("diamond_ore"),
        id::REDSTONE_ORE => normal("redstone_ore"),
        id::EMERALD_ORE => normal("emerald_ore"),
        id::CLAY => normal("clay"),
        id::MELON_BLOCK => normal("melon_block"),
        id::PACKED_ICE => normal("packed_ice"),
        // CACTUS and REEDS ignore AGE.
        id::CACTUS => normal("cactus"),
        id::REEDS => normal("reeds"),
        // withName(VARIANT).withSuffix("_log"); BlockLog.EnumAxis by meta & 12.
        id::LOG | id::LOG2 => {
            let wood = if block == id::LOG { meta & 3 } else { 4 + (meta & 3) };
            let axis = ["y", "x", "z", "none"][meta >> 2];
            (format!("{}_log", WOODS.get(wood)?), format!("axis={axis}"))
        }
        // withName(VARIANT).withSuffix("_leaves"), ignoring CHECK_DECAY and DECAYABLE.
        id::LEAVES | id::LEAVES2 => {
            let wood = if block == id::LEAVES { meta & 3 } else { 4 + (meta & 3) };
            normal(&format!("{}_leaves", WOODS.get(wood)?))
        }
        id::TALLGRASS => normal(["dead_bush", "tall_grass", "fern"].get(meta)?),
        id::DEADBUSH => normal("dead_bush"),
        id::YELLOW_FLOWER => normal("dandelion"),
        id::RED_FLOWER => normal(RED_FLOWERS.get(meta)?),
        id::BROWN_MUSHROOM => normal("brown_mushroom"),
        id::RED_MUSHROOM => normal("red_mushroom"),
        id::MONSTER_EGG => normal(&format!("{}_monster_egg", ["stone", "cobblestone", "stone_brick"].get(meta)?)),
        id::SNOW_LAYER => ("snow_layer".into(), format!("layers={}", (meta & 7) + 1)),
        id::BROWN_MUSHROOM_BLOCK | id::RED_MUSHROOM_BLOCK => {
            let v = MUSHROOM_BLOCK[meta];
            if v.is_empty() {
                return None;
            }
            let file = if block == id::BROWN_MUSHROOM_BLOCK { "brown_mushroom_block" } else { "red_mushroom_block" };
            (file.into(), format!("variant={v}"))
        }
        // BlockVine: SOUTH 1, WEST 2, NORTH 4, EAST 8.
        id::VINE => (
            "vine".into(),
            format!("east={},north={},south={},up={flag},west={}", meta & 8 != 0, meta & 4 != 0, meta & 1 != 0, meta & 2 != 0),
        ),
        // BlockCocoa: FACING byHorizontalIndex(meta & 3), AGE (meta & 15) >> 2.
        id::COCOA => ("cocoa".into(), format!("age={},facing={}", meta >> 2, BY_HORIZONTAL[meta & 3])),
        // withName(VARIANT).ignore(FACING).
        id::DOUBLE_PLANT => {
            let half = if meta & 8 != 0 { "upper" } else { "lower" };
            (DOUBLE_PLANTS.get(meta & 7)?.to_string(), format!("half={half}"))
        }
        // BlockTorch.getStateFromMeta: EAST 1, WEST 2, SOUTH 3, NORTH 4, else UP.
        id::TORCH => ("torch".into(), format!("facing={}", ["up", "east", "west", "south", "north", "up"][meta.min(5)])),
        // BlockPumpkin: FACING byHorizontalIndex(meta).
        id::PUMPKIN => ("pumpkin".into(), format!("facing={}", BY_HORIZONTAL[meta & 3])),
        // BlockModelShapes: the half slabs withName(VARIANT).withSuffix("_slab"); the double
        // stone slabs by VARIANT with "_double_slab" and SEAMLESS as variant "all" or "normal"; the
        // double wooden slab withName(VARIANT).withSuffix("_double_slab"); purpur ignores nothing.
        id::STONE_SLAB => (format!("{}_slab", STONE_SLABS[meta & 7]), slab_half(meta)),
        id::DOUBLE_STONE_SLAB => (format!("{}_double_slab", STONE_SLABS[meta & 7]), seamless(meta)),
        id::STONE_SLAB2 if meta & 7 == 0 => ("red_sandstone_slab".into(), slab_half(meta)),
        id::DOUBLE_STONE_SLAB2 if meta & 7 == 0 => ("red_sandstone_double_slab".into(), seamless(meta)),
        id::WOODEN_SLAB => (format!("{}_slab", WOODS.get(meta & 7)?), slab_half(meta)),
        id::DOUBLE_WOODEN_SLAB => (format!("{}_double_slab", WOODS.get(meta & 7)?), "normal".into()),
        id::PURPUR_SLAB => ("purpur_slab".into(), format!("{},variant=default", slab_half(meta))),
        id::PURPUR_DOUBLE_SLAB => ("purpur_double_slab".into(), "variant=default".into()),
        _ => return None,
    })
}

/// `BlockSlab.HALF` (meta & 8) as a variant key.
fn slab_half(meta: usize) -> String {
    format!("half={}", if meta & 8 != 0 { "top" } else { "bottom" })
}

/// `BlockStoneSlab.SEAMLESS` (meta & 8): the double slab's variant.
fn seamless(meta: usize) -> String {
    (if meta & 8 != 0 { "all" } else { "normal" }).into()
}

/// Every state with a model that generation, population or the player can place, in the forms
/// the mesher asks for (double plant upper halves carry their variant).
pub fn generated_states() -> Vec<u16> {
    use worldgen::blocks::*;
    let mut v = vec![
        STONE, GRASS, DIRT, COARSE_DIRT, PODZOL, BEDROCK, SAND, RED_SAND, GRAVEL, SANDSTONE, RED_SANDSTONE, ICE, SNOW,
        MYCELIUM, WATERLILY, HARDENED_CLAY,
    ];
    v.extend((1..7).map(|m| state(id::STONE, m)));
    v.extend((0..16).map(stained_hardened_clay));
    for b in [
        id::COBBLESTONE,
        id::MOSSY_COBBLESTONE,
        id::OBSIDIAN,
        id::MOB_SPAWNER,
        id::GLASS,
        id::BRICK_BLOCK,
        id::COAL_ORE,
        id::IRON_ORE,
        id::GOLD_ORE,
        id::LAPIS_ORE,
        id::DIAMOND_ORE,
        id::REDSTONE_ORE,
        id::EMERALD_ORE,
        id::CLAY,
        id::MELON_BLOCK,
        id::PACKED_ICE,
        id::DEADBUSH,
        id::YELLOW_FLOWER,
        id::BROWN_MUSHROOM,
        id::RED_MUSHROOM,
    ] {
        v.push(state(b, 0));
    }
    for b in [id::LOG, id::LEAVES, id::CACTUS, id::REEDS, id::VINE, id::SNOW_LAYER] {
        v.extend((0..16).map(|m| state(b, m)));
    }
    for b in [id::LOG2, id::LEAVES2] {
        v.extend((0..16).filter(|m| m & 3 < 2).map(|m| state(b, m)));
    }
    v.extend((0..3).map(|m| state(id::TALLGRASS, m)));
    v.extend((0..9).map(|m| state(id::RED_FLOWER, m)));
    v.extend((0..3).map(|m| state(id::MONSTER_EGG, m)));
    v.extend((0..6).map(|m| state(id::PLANKS, m)));
    v.extend((0..4).map(|m| state(id::STONEBRICK, m)));
    for b in [id::BROWN_MUSHROOM_BLOCK, id::RED_MUSHROOM_BLOCK] {
        v.extend((0..16).filter(|&m| !MUSHROOM_BLOCK[m as usize].is_empty()).map(|m| state(b, m)));
    }
    v.extend((0..12).map(|m| state(id::COCOA, m)));
    v.extend((0..4).map(|m| state(id::PUMPKIN, m)));
    v.extend((1..6).map(|m| state(id::TORCH, m)));
    for (half, double, variants) in [
        (id::STONE_SLAB, id::DOUBLE_STONE_SLAB, 8),
        (id::WOODEN_SLAB, id::DOUBLE_WOODEN_SLAB, 6),
        (id::STONE_SLAB2, id::DOUBLE_STONE_SLAB2, 1),
        (id::PURPUR_SLAB, id::PURPUR_DOUBLE_SLAB, 1),
    ] {
        for m in 0..variants {
            v.extend([state(half, m), state(half, m | 8), state(double, m)]);
            // The wooden double slab has no SEAMLESS bit.
            if double != id::DOUBLE_WOODEN_SLAB {
                v.push(state(double, m | 8));
            }
        }
    }
    v.extend((0..6).chain(8..14).map(|m| state(id::DOUBLE_PLANT, m)));
    v
}

#[cfg(test)]
mod tests {
    use super::*;
    use worldgen::blocks::state;

    #[test]
    fn keys_follow_the_state_mappers() {
        let k = |b, m, f| blockstate_of(state(b, m), f).unwrap();
        assert_eq!(k(id::LOG, 4, false), ("oak_log".into(), "axis=x".into()));
        assert_eq!(k(id::LOG2, 9, false), ("dark_oak_log".into(), "axis=z".into()));
        assert_eq!(k(id::LEAVES, 10, false), ("birch_leaves".into(), "normal".into()));
        assert_eq!(k(id::VINE, 4, true), ("vine".into(), "east=false,north=true,south=false,up=true,west=false".into()));
        assert_eq!(k(id::COCOA, 9, false), ("cocoa".into(), "age=2,facing=west".into()));
        assert_eq!(k(id::DOUBLE_PLANT, 10, false), ("double_grass".into(), "half=upper".into()));
        assert_eq!(k(id::SNOW_LAYER, 0, false), ("snow_layer".into(), "layers=1".into()));
        assert_eq!(k(id::STONE_SLAB, 8, false), ("stone_slab".into(), "half=top".into()));
        assert_eq!(k(id::DOUBLE_STONE_SLAB, 3, false), ("cobblestone_double_slab".into(), "normal".into()));
        assert_eq!(k(id::DOUBLE_STONE_SLAB, 8, false), ("stone_double_slab".into(), "all".into()));
        assert_eq!(k(id::WOODEN_SLAB, 13, false), ("dark_oak_slab".into(), "half=top".into()));
        assert_eq!(k(id::DOUBLE_WOODEN_SLAB, 2, false), ("birch_double_slab".into(), "normal".into()));
        assert_eq!(k(id::PURPUR_SLAB, 0, false), ("purpur_slab".into(), "half=bottom,variant=default".into()));
        assert_eq!(k(id::PURPUR_DOUBLE_SLAB, 0, false), ("purpur_double_slab".into(), "variant=default".into()));
        assert_eq!(k(id::RED_MUSHROOM_BLOCK, 15, false), ("red_mushroom_block".into(), "variant=all_stem".into()));
        assert_eq!(k(id::PUMPKIN, 1, false), ("pumpkin".into(), "facing=west".into()));
        assert_eq!(k(id::STONE_SLAB, 1, false), ("sandstone_slab".into(), "half=bottom".into()));
        assert_eq!(k(id::PLANKS, 5, false), ("dark_oak_planks".into(), "normal".into()));
        assert_eq!(k(id::STONEBRICK, 3, false), ("chiseled_stonebrick".into(), "normal".into()));
        assert!(blockstate_of(state(id::STONEBRICK, 4), false).is_none());
        assert!(blockstate_of(state(id::CHEST, 2), false).is_none());
    }
}
