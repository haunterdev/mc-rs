//! The biome registry from `Biome.registerBiomes`, with the per-class data terrain generation reads.

use crate::blocks;

/// Biome ids, named after the `Biomes` constants.
pub mod ids {
    pub const OCEAN: i32 = 0;
    pub const PLAINS: i32 = 1;
    pub const DESERT: i32 = 2;
    pub const EXTREME_HILLS: i32 = 3;
    pub const FOREST: i32 = 4;
    pub const TAIGA: i32 = 5;
    pub const SWAMPLAND: i32 = 6;
    pub const RIVER: i32 = 7;
    pub const HELL: i32 = 8;
    pub const SKY: i32 = 9;
    pub const FROZEN_OCEAN: i32 = 10;
    pub const FROZEN_RIVER: i32 = 11;
    pub const ICE_PLAINS: i32 = 12;
    pub const ICE_MOUNTAINS: i32 = 13;
    pub const MUSHROOM_ISLAND: i32 = 14;
    pub const MUSHROOM_ISLAND_SHORE: i32 = 15;
    pub const BEACH: i32 = 16;
    pub const DESERT_HILLS: i32 = 17;
    pub const FOREST_HILLS: i32 = 18;
    pub const TAIGA_HILLS: i32 = 19;
    pub const EXTREME_HILLS_EDGE: i32 = 20;
    pub const JUNGLE: i32 = 21;
    pub const JUNGLE_HILLS: i32 = 22;
    pub const JUNGLE_EDGE: i32 = 23;
    pub const DEEP_OCEAN: i32 = 24;
    pub const STONE_BEACH: i32 = 25;
    pub const COLD_BEACH: i32 = 26;
    pub const BIRCH_FOREST: i32 = 27;
    pub const BIRCH_FOREST_HILLS: i32 = 28;
    pub const ROOFED_FOREST: i32 = 29;
    pub const COLD_TAIGA: i32 = 30;
    pub const COLD_TAIGA_HILLS: i32 = 31;
    pub const REDWOOD_TAIGA: i32 = 32;
    pub const REDWOOD_TAIGA_HILLS: i32 = 33;
    pub const EXTREME_HILLS_WITH_TREES: i32 = 34;
    pub const SAVANNA: i32 = 35;
    pub const SAVANNA_PLATEAU: i32 = 36;
    pub const MESA: i32 = 37;
    pub const MESA_ROCK: i32 = 38;
    pub const MESA_CLEAR_ROCK: i32 = 39;
    pub const VOID: i32 = 127;
    pub const MUTATED_PLAINS: i32 = 129;
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HillsType {
    Normal,
    ExtraTrees,
    Mutated,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TaigaType {
    Normal,
    Mega,
    MegaSpruce,
}

/// The Java class of a biome instance, with the constructor arguments terrain generation reads.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Ocean,
    Plains,
    Desert,
    Hills(HillsType),
    Forest,
    ForestMutated,
    Taiga(TaigaType),
    Swamp,
    River,
    Hell,
    End,
    Snow { super_icy: bool },
    MushroomIsland,
    Beach,
    Jungle,
    StoneBeach,
    Savanna,
    SavannaMutated,
    Mesa { bryce_pillars: bool, has_forest: bool },
    Void,
}

/// `Biome.getBiomeClass()`: the class itself, except `BiomeForest` and `BiomeSavanna`, which
/// report their own class for their mutated subclasses.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BiomeClass {
    Ocean,
    Plains,
    Desert,
    Hills,
    Forest,
    Taiga,
    Swamp,
    River,
    Hell,
    End,
    Snow,
    MushroomIsland,
    Beach,
    Jungle,
    StoneBeach,
    Savanna,
    Mesa,
    Void,
}

impl BiomeClass {
    /// `getBiomeClass().getSimpleName()`
    pub fn simple_name(self) -> &'static str {
        match self {
            BiomeClass::Ocean => "BiomeOcean",
            BiomeClass::Plains => "BiomePlains",
            BiomeClass::Desert => "BiomeDesert",
            BiomeClass::Hills => "BiomeHills",
            BiomeClass::Forest => "BiomeForest",
            BiomeClass::Taiga => "BiomeTaiga",
            BiomeClass::Swamp => "BiomeSwamp",
            BiomeClass::River => "BiomeRiver",
            BiomeClass::Hell => "BiomeHell",
            BiomeClass::End => "BiomeEnd",
            BiomeClass::Snow => "BiomeSnow",
            BiomeClass::MushroomIsland => "BiomeMushroomIsland",
            BiomeClass::Beach => "BiomeBeach",
            BiomeClass::Jungle => "BiomeJungle",
            BiomeClass::StoneBeach => "BiomeStoneBeach",
            BiomeClass::Savanna => "BiomeSavanna",
            BiomeClass::Mesa => "BiomeMesa",
            BiomeClass::Void => "BiomeVoid",
        }
    }
}

/// `Biome.TempCategory`
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TempCategory {
    Ocean,
    Cold,
    Medium,
    Warm,
}

impl TempCategory {
    pub fn name(self) -> &'static str {
        match self {
            TempCategory::Ocean => "OCEAN",
            TempCategory::Cold => "COLD",
            TempCategory::Medium => "MEDIUM",
            TempCategory::Warm => "WARM",
        }
    }
}

/// One registered biome: `Biome.BiomeProperties` plus the class and its constructor defaults.
#[derive(Clone, Copy, Debug)]
pub struct Biome {
    pub id: u8,
    /// Registry name.
    pub name: &'static str,
    pub kind: Kind,
    pub base_height: f32,
    pub height_variation: f32,
    pub temperature: f32,
    pub rainfall: f32,
    /// `isSnowyBiome()` (`BiomeProperties.setSnowEnabled`).
    pub snowy: bool,
    /// `baseBiomeRegName` resolved to an id; set for mutations.
    pub base_biome: Option<u8>,
    /// `topBlock` as the constructor leaves it.
    pub top_block: u16,
    /// `fillerBlock` as the constructor leaves it.
    pub filler_block: u16,
}

impl Biome {
    /// `Biome.getBiomeClass()`
    pub const fn biome_class(&self) -> BiomeClass {
        match self.kind {
            Kind::Ocean => BiomeClass::Ocean,
            Kind::Plains => BiomeClass::Plains,
            Kind::Desert => BiomeClass::Desert,
            Kind::Hills(_) => BiomeClass::Hills,
            Kind::Forest | Kind::ForestMutated => BiomeClass::Forest,
            Kind::Taiga(_) => BiomeClass::Taiga,
            Kind::Swamp => BiomeClass::Swamp,
            Kind::River => BiomeClass::River,
            Kind::Hell => BiomeClass::Hell,
            Kind::End => BiomeClass::End,
            Kind::Snow { .. } => BiomeClass::Snow,
            Kind::MushroomIsland => BiomeClass::MushroomIsland,
            Kind::Beach => BiomeClass::Beach,
            Kind::Jungle => BiomeClass::Jungle,
            Kind::StoneBeach => BiomeClass::StoneBeach,
            Kind::Savanna | Kind::SavannaMutated => BiomeClass::Savanna,
            Kind::Mesa { .. } => BiomeClass::Mesa,
            Kind::Void => BiomeClass::Void,
        }
    }

    /// `Biome.isMutation()`
    pub const fn is_mutation(&self) -> bool {
        self.base_biome.is_some()
    }

    /// `Biome.getTempCategory()`, with the `BiomeOcean` override. The comparisons widen the float
    /// temperature to double, as vanilla does.
    pub fn temp_category(&self) -> TempCategory {
        if matches!(self.kind, Kind::Ocean) {
            TempCategory::Ocean
        } else if (self.temperature as f64) < 0.2 {
            TempCategory::Cold
        } else if (self.temperature as f64) < 1.0 {
            TempCategory::Medium
        } else {
            TempCategory::Warm
        }
    }
}

const GRASS: u16 = blocks::GRASS;
const DIRT: u16 = blocks::DIRT;

/// `BiomeProperties` defaults: base height 0.1, variation 0.2, temperature 0.5, rainfall 0.5.
const fn b(
    id: u8,
    name: &'static str,
    kind: Kind,
    base_height: f32,
    height_variation: f32,
    temperature: f32,
    rainfall: f32,
    snowy: bool,
    base_biome: Option<u8>,
) -> Biome {
    let (top_block, filler_block) = match kind {
        Kind::Desert | Kind::Beach => (blocks::SAND, blocks::SAND),
        Kind::End => (blocks::DIRT, blocks::DIRT),
        Kind::StoneBeach => (blocks::STONE, blocks::STONE),
        Kind::MushroomIsland => (blocks::MYCELIUM, DIRT),
        Kind::Snow { super_icy: true } => (blocks::SNOW, DIRT),
        Kind::Mesa { .. } => (blocks::RED_SAND, blocks::stained_hardened_clay(blocks::dye::WHITE)),
        _ => (GRASS, DIRT),
    };
    Biome {
        id,
        name,
        kind,
        base_height,
        height_variation,
        temperature,
        rainfall,
        snowy,
        base_biome,
        top_block,
        filler_block,
    }
}

use HillsType as H;
use Kind as K;
use TaigaType as T;

/// `Biome.registerBiomes()`, in registration order.
#[rustfmt::skip]
pub const DEFS: [Biome; 62] = [
    b(0, "ocean", K::Ocean, -1.0, 0.1, 0.5, 0.5, false, None),
    b(1, "plains", K::Plains, 0.125, 0.05, 0.8, 0.4, false, None),
    b(2, "desert", K::Desert, 0.125, 0.05, 2.0, 0.0, false, None),
    b(3, "extreme_hills", K::Hills(H::Normal), 1.0, 0.5, 0.2, 0.3, false, None),
    b(4, "forest", K::Forest, 0.1, 0.2, 0.7, 0.8, false, None),
    b(5, "taiga", K::Taiga(T::Normal), 0.2, 0.2, 0.25, 0.8, false, None),
    b(6, "swampland", K::Swamp, -0.2, 0.1, 0.8, 0.9, false, None),
    b(7, "river", K::River, -0.5, 0.0, 0.5, 0.5, false, None),
    b(8, "hell", K::Hell, 0.1, 0.2, 2.0, 0.0, false, None),
    b(9, "sky", K::End, 0.1, 0.2, 0.5, 0.5, false, None),
    b(10, "frozen_ocean", K::Ocean, -1.0, 0.1, 0.0, 0.5, true, None),
    b(11, "frozen_river", K::River, -0.5, 0.0, 0.0, 0.5, true, None),
    b(12, "ice_flats", K::Snow { super_icy: false }, 0.125, 0.05, 0.0, 0.5, true, None),
    b(13, "ice_mountains", K::Snow { super_icy: false }, 0.45, 0.3, 0.0, 0.5, true, None),
    b(14, "mushroom_island", K::MushroomIsland, 0.2, 0.3, 0.9, 1.0, false, None),
    b(15, "mushroom_island_shore", K::MushroomIsland, 0.0, 0.025, 0.9, 1.0, false, None),
    b(16, "beaches", K::Beach, 0.0, 0.025, 0.8, 0.4, false, None),
    b(17, "desert_hills", K::Desert, 0.45, 0.3, 2.0, 0.0, false, None),
    b(18, "forest_hills", K::Forest, 0.45, 0.3, 0.7, 0.8, false, None),
    b(19, "taiga_hills", K::Taiga(T::Normal), 0.45, 0.3, 0.25, 0.8, false, None),
    b(20, "smaller_extreme_hills", K::Hills(H::ExtraTrees), 0.8, 0.3, 0.2, 0.3, false, None),
    b(21, "jungle", K::Jungle, 0.1, 0.2, 0.95, 0.9, false, None),
    b(22, "jungle_hills", K::Jungle, 0.45, 0.3, 0.95, 0.9, false, None),
    b(23, "jungle_edge", K::Jungle, 0.1, 0.2, 0.95, 0.8, false, None),
    b(24, "deep_ocean", K::Ocean, -1.8, 0.1, 0.5, 0.5, false, None),
    b(25, "stone_beach", K::StoneBeach, 0.1, 0.8, 0.2, 0.3, false, None),
    b(26, "cold_beach", K::Beach, 0.0, 0.025, 0.05, 0.3, true, None),
    b(27, "birch_forest", K::Forest, 0.1, 0.2, 0.6, 0.6, false, None),
    b(28, "birch_forest_hills", K::Forest, 0.45, 0.3, 0.6, 0.6, false, None),
    b(29, "roofed_forest", K::Forest, 0.1, 0.2, 0.7, 0.8, false, None),
    b(30, "taiga_cold", K::Taiga(T::Normal), 0.2, 0.2, -0.5, 0.4, true, None),
    b(31, "taiga_cold_hills", K::Taiga(T::Normal), 0.45, 0.3, -0.5, 0.4, true, None),
    b(32, "redwood_taiga", K::Taiga(T::Mega), 0.2, 0.2, 0.3, 0.8, false, None),
    b(33, "redwood_taiga_hills", K::Taiga(T::Mega), 0.45, 0.3, 0.3, 0.8, false, None),
    b(34, "extreme_hills_with_trees", K::Hills(H::ExtraTrees), 1.0, 0.5, 0.2, 0.3, false, None),
    b(35, "savanna", K::Savanna, 0.125, 0.05, 1.2, 0.0, false, None),
    b(36, "savanna_rock", K::Savanna, 1.5, 0.025, 1.0, 0.0, false, None),
    b(37, "mesa", K::Mesa { bryce_pillars: false, has_forest: false }, 0.1, 0.2, 2.0, 0.0, false, None),
    b(38, "mesa_rock", K::Mesa { bryce_pillars: false, has_forest: true }, 1.5, 0.025, 2.0, 0.0, false, None),
    b(39, "mesa_clear_rock", K::Mesa { bryce_pillars: false, has_forest: false }, 1.5, 0.025, 2.0, 0.0, false, None),
    b(127, "void", K::Void, 0.1, 0.2, 0.5, 0.5, false, None),
    b(129, "mutated_plains", K::Plains, 0.125, 0.05, 0.8, 0.4, false, Some(1)),
    b(130, "mutated_desert", K::Desert, 0.225, 0.25, 2.0, 0.0, false, Some(2)),
    b(131, "mutated_extreme_hills", K::Hills(H::Mutated), 1.0, 0.5, 0.2, 0.3, false, Some(3)),
    b(132, "mutated_forest", K::Forest, 0.1, 0.4, 0.7, 0.8, false, Some(4)),
    b(133, "mutated_taiga", K::Taiga(T::Normal), 0.3, 0.4, 0.25, 0.8, false, Some(5)),
    b(134, "mutated_swampland", K::Swamp, -0.1, 0.3, 0.8, 0.9, false, Some(6)),
    b(140, "mutated_ice_flats", K::Snow { super_icy: true }, 0.425, 0.45000002, 0.0, 0.5, true, Some(12)),
    b(149, "mutated_jungle", K::Jungle, 0.2, 0.4, 0.95, 0.9, false, Some(21)),
    b(151, "mutated_jungle_edge", K::Jungle, 0.2, 0.4, 0.95, 0.8, false, Some(23)),
    b(155, "mutated_birch_forest", K::ForestMutated, 0.2, 0.4, 0.6, 0.6, false, Some(27)),
    b(156, "mutated_birch_forest_hills", K::ForestMutated, 0.55, 0.5, 0.6, 0.6, false, Some(28)),
    b(157, "mutated_roofed_forest", K::Forest, 0.2, 0.4, 0.7, 0.8, false, Some(29)),
    b(158, "mutated_taiga_cold", K::Taiga(T::Normal), 0.3, 0.4, -0.5, 0.4, true, Some(30)),
    b(160, "mutated_redwood_taiga", K::Taiga(T::MegaSpruce), 0.2, 0.2, 0.25, 0.8, false, Some(32)),
    b(161, "mutated_redwood_taiga_hills", K::Taiga(T::MegaSpruce), 0.2, 0.2, 0.25, 0.8, false, Some(33)),
    b(162, "mutated_extreme_hills_with_trees", K::Hills(H::Mutated), 1.0, 0.5, 0.2, 0.3, false, Some(34)),
    b(163, "mutated_savanna", K::SavannaMutated, 0.3625, 1.225, 1.1, 0.0, false, Some(35)),
    b(164, "mutated_savanna_rock", K::SavannaMutated, 1.05, 1.2125001, 1.0, 0.0, false, Some(36)),
    b(165, "mutated_mesa", K::Mesa { bryce_pillars: true, has_forest: false }, 0.1, 0.2, 2.0, 0.0, false, Some(37)),
    b(166, "mutated_mesa_rock", K::Mesa { bryce_pillars: false, has_forest: true }, 0.45, 0.3, 2.0, 0.0, false, Some(38)),
    b(167, "mutated_mesa_clear_rock", K::Mesa { bryce_pillars: false, has_forest: false }, 0.45, 0.3, 2.0, 0.0, false, Some(39)),
];

const fn build_table() -> [Option<Biome>; 256] {
    let mut t = [None; 256];
    let mut i = 0;
    while i < DEFS.len() {
        t[DEFS[i].id as usize] = Some(DEFS[i]);
        i += 1;
    }
    t
}

/// `MUTATION_TO_BASE_ID_MAP`: base id to mutation id, -1 where there is none.
const fn build_mutations() -> [i32; 256] {
    let mut t = [-1i32; 256];
    let mut i = 0;
    while i < DEFS.len() {
        if let Some(base) = DEFS[i].base_biome {
            t[base as usize] = DEFS[i].id as i32;
        }
        i += 1;
    }
    t
}

static TABLE: [Option<Biome>; 256] = build_table();
static MUTATIONS: [i32; 256] = build_mutations();

/// `Biome.getBiome(int)` / `Biome.getBiomeForId(int)`: `None` for an unregistered or out-of-range id.
#[inline]
pub fn get(id: i32) -> Option<&'static Biome> {
    if (0..256).contains(&id) {
        TABLE[id as usize].as_ref()
    } else {
        None
    }
}

/// `Biome.getBiome(id, Biomes.DEFAULT)` as `BiomeProvider` uses it: unknown ids become ocean.
#[inline]
pub fn get_or_default(id: i32) -> &'static Biome {
    get(id).unwrap_or_else(|| TABLE[ids::OCEAN as usize].as_ref().unwrap())
}

/// `Biome.getMutationForBiome(Biome.getBiome(id))` as an id.
#[inline]
pub fn mutation_for(id: i32) -> Option<i32> {
    if (0..256).contains(&id) && MUTATIONS[id as usize] >= 0 {
        Some(MUTATIONS[id as usize])
    } else {
        None
    }
}

/// `GenLayer.isBiomeOceanic`
#[inline]
pub fn is_oceanic(id: i32) -> bool {
    id == ids::OCEAN || id == ids::DEEP_OCEAN || id == ids::FROZEN_OCEAN
}

/// Every registered biome, in id order.
pub fn all() -> impl Iterator<Item = &'static Biome> {
    TABLE.iter().filter_map(|b| b.as_ref())
}
