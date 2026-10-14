//! The population half of each `Biome` subclass: decorator counts from the constructor,
//! `getRandomTreeFeature`, `pickRandomFlower`, `getRandomWorldGenForGrass`, `decorate`, the
//! creature spawn list and `getSpawningChance`.

use std::sync::LazyLock;

use super::decorator::{self, Decorator};
use super::features::{self, plant};
use super::features2;
use super::state::Pos;
use super::trees::{Tree, JUNGLE_LEAF, JUNGLE_LOG, OAK_LEAF, TREE_FEATURE};
use super::world::PopWorld;
use crate::biome::{Biome, HillsType, Kind, TaigaType};
use crate::blocks::{self, id};
use crate::java_random::JavaRandom;
use crate::noise::perlin::NoiseGeneratorPerlin;

/// `Biome.GRASS_COLOR_NOISE`
static GRASS_COLOR_NOISE: LazyLock<NoiseGeneratorPerlin> =
    LazyLock::new(|| NoiseGeneratorPerlin::new(&mut JavaRandom::new(2345), 1));

/// `BiomeForest.Type`, which the registry passes per biome id.
#[derive(Clone, Copy, PartialEq, Eq)]
enum ForestType {
    Normal,
    Flower,
    Birch,
    Roofed,
}

fn forest_type(b: &Biome) -> ForestType {
    match b.kind {
        Kind::ForestMutated => ForestType::Birch,
        _ => match b.id {
            132 => ForestType::Flower,
            27 | 28 => ForestType::Birch,
            29 | 157 => ForestType::Roofed,
            _ => ForestType::Normal,
        },
    }
}

/// `BiomeJungle.isEdge`
fn jungle_edge(b: &Biome) -> bool {
    matches!(b.id, 23 | 151)
}

/// `BiomePlains.sunflowers`
fn plains_sunflowers(b: &Biome) -> bool {
    b.id == 129
}

/// The decorator counts as each constructor leaves them.
pub fn decorator_for(b: &Biome) -> Decorator {
    let mut d = Decorator::default();
    match b.kind {
        Kind::Beach | Kind::StoneBeach => {
            d.trees = -999;
            d.dead_bush = 0;
            d.reeds = 0;
            d.cacti = 0;
        }
        Kind::Desert => {
            d.trees = -999;
            d.dead_bush = 2;
            d.reeds = 50;
            d.cacti = 10;
        }
        Kind::Forest | Kind::ForestMutated => {
            d.trees = 10;
            d.grass = 2;
            match forest_type(b) {
                ForestType::Flower => {
                    d.trees = 6;
                    d.flowers = 100;
                    d.grass = 1;
                }
                ForestType::Roofed => d.trees = -999,
                _ => {}
            }
        }
        Kind::Hills(t) => {
            if t == HillsType::ExtraTrees {
                d.trees = 3;
            }
        }
        Kind::Jungle => {
            d.trees = if jungle_edge(b) { 2 } else { 50 };
            d.grass = 25;
            d.flowers = 4;
        }
        Kind::Mesa { has_forest, .. } => {
            d.trees = -999;
            d.dead_bush = 20;
            d.reeds = 3;
            d.cacti = 5;
            d.flowers = 0;
            if has_forest {
                d.trees = 5;
            }
            d.mesa_gold = true;
        }
        Kind::MushroomIsland => {
            d.trees = -100;
            d.flowers = -100;
            d.grass = -100;
            d.mushrooms = 1;
            d.big_mushrooms = 1;
        }
        Kind::Plains => {
            d.trees = 0;
            d.extra_tree_chance = 0.05;
            d.flowers = 4;
            d.grass = 10;
        }
        Kind::Savanna | Kind::SavannaMutated => {
            d.trees = 1;
            d.flowers = 4;
            d.grass = 20;
            if b.kind == Kind::SavannaMutated {
                d.trees = 2;
                d.flowers = 2;
                d.grass = 5;
            }
        }
        Kind::Swamp => {
            d.trees = 2;
            d.flowers = 1;
            d.dead_bush = 1;
            d.mushrooms = 8;
            d.reeds = 10;
            d.clay = 1;
            d.waterlily = 4;
            d.sand_patches = 0;
            d.gravel_patches = 0;
            d.grass = 5;
        }
        Kind::Taiga(t) => {
            d.trees = 10;
            if t == TaigaType::Normal {
                d.grass = 1;
                d.mushrooms = 1;
            } else {
                d.grass = 7;
                d.dead_bush = 1;
                d.mushrooms = 3;
            }
        }
        // Ocean, River, Snow: the base constructor's decorator.
        // DROPPED: Hell, End and Void decorators; the overworld generator never reaches them.
        _ => {}
    }
    d
}

/// `Biome.getRandomTreeFeature` and its overrides.
pub fn random_tree(b: &Biome, rand: &mut JavaRandom) -> Tree {
    match b.kind {
        Kind::Forest => {
            let t = forest_type(b);
            if t == ForestType::Roofed && rand.next_int_bounded(3) > 0 {
                Tree::Canopy
            } else if t != ForestType::Birch && rand.next_int_bounded(5) != 0 {
                base_tree(rand)
            } else {
                Tree::Birch { extra: false }
            }
        }
        Kind::ForestMutated => {
            if rand.next_boolean() {
                Tree::Birch { extra: true }
            } else {
                Tree::Birch { extra: false }
            }
        }
        Kind::Hills(_) => {
            if rand.next_int_bounded(3) > 0 {
                Tree::Taiga2
            } else {
                base_tree(rand)
            }
        }
        Kind::Jungle => {
            if rand.next_int_bounded(10) == 0 {
                Tree::Big
            } else if rand.next_int_bounded(2) == 0 {
                Tree::Shrub { wood: JUNGLE_LOG, leaves: OAK_LEAF }
            } else if !jungle_edge(b) && rand.next_int_bounded(3) == 0 {
                Tree::MegaJungle { base: 10, extra: 20, wood: JUNGLE_LOG, leaves: JUNGLE_LEAF }
            } else {
                Tree::Trees { min_height: 4 + rand.next_int_bounded(7), wood: JUNGLE_LOG, leaves: JUNGLE_LEAF, vines: true }
            }
        }
        Kind::Mesa { .. } => TREE_FEATURE,
        Kind::Plains => {
            if rand.next_int_bounded(3) == 0 {
                Tree::Big
            } else {
                TREE_FEATURE
            }
        }
        Kind::Savanna | Kind::SavannaMutated => {
            if rand.next_int_bounded(5) > 0 {
                Tree::Savanna
            } else {
                TREE_FEATURE
            }
        }
        Kind::Snow { .. } => Tree::Taiga2,
        Kind::Swamp => Tree::Swamp,
        Kind::Taiga(t) => {
            if (t == TaigaType::Mega || t == TaigaType::MegaSpruce) && rand.next_int_bounded(3) == 0 {
                if t != TaigaType::MegaSpruce && rand.next_int_bounded(13) != 0 {
                    Tree::MegaPine { use_base_height: false }
                } else {
                    Tree::MegaPine { use_base_height: true }
                }
            } else if rand.next_int_bounded(3) == 0 {
                Tree::Taiga1
            } else {
                Tree::Taiga2
            }
        }
        _ => base_tree(rand),
    }
}

/// `Biome.getRandomTreeFeature`
fn base_tree(rand: &mut JavaRandom) -> Tree {
    if rand.next_int_bounded(10) == 0 {
        Tree::Big
    } else {
        TREE_FEATURE
    }
}

/// `BlockFlower.EnumFlowerType` ordinals.
mod flower {
    pub const DANDELION: u16 = 0;
    pub const POPPY: u16 = 1;
    pub const BLUE_ORCHID: u16 = 2;
    pub const HOUSTONIA: u16 = 4;
    pub const RED_TULIP: u16 = 5;
    pub const ORANGE_TULIP: u16 = 6;
    pub const WHITE_TULIP: u16 = 7;
    pub const PINK_TULIP: u16 = 8;
    pub const OXEYE_DAISY: u16 = 9;
    /// `EnumFlowerType.values().length`
    pub const COUNT: u16 = 10;
}

/// The state `WorldGenFlowers.setGeneratedBlock` builds: DANDELION is the only YELLOW_FLOWER type,
/// the rest are RED_FLOWER with meta `ordinal - 1`.
fn flower_state(ordinal: u16) -> u16 {
    if ordinal == flower::DANDELION {
        blocks::state(id::YELLOW_FLOWER, 0)
    } else {
        blocks::state(id::RED_FLOWER, ordinal - 1)
    }
}

/// `Biome.pickRandomFlower` and its overrides, as the flower block state.
pub fn pick_flower(b: &Biome, rand: &mut JavaRandom, pos: Pos) -> u16 {
    let ordinal = match b.kind {
        Kind::Forest if forest_type(b) == ForestType::Flower => {
            let d0 = ((1.0 + GRASS_COLOR_NOISE.get_value(pos.x as f64 / 48.0, pos.z as f64 / 48.0)) / 2.0).clamp(0.0, 0.9999);
            let t = (d0 * flower::COUNT as f64) as u16;
            if t == flower::BLUE_ORCHID {
                flower::POPPY
            } else {
                t
            }
        }
        Kind::Plains => {
            let d0 = GRASS_COLOR_NOISE.get_value(pos.x as f64 / 200.0, pos.z as f64 / 200.0);
            if d0 < -0.8 {
                match rand.next_int_bounded(4) {
                    0 => flower::ORANGE_TULIP,
                    1 => flower::RED_TULIP,
                    2 => flower::PINK_TULIP,
                    _ => flower::WHITE_TULIP,
                }
            } else if rand.next_int_bounded(3) > 0 {
                match rand.next_int_bounded(3) {
                    0 => flower::POPPY,
                    1 => flower::HOUSTONIA,
                    _ => flower::OXEYE_DAISY,
                }
            } else {
                flower::DANDELION
            }
        }
        Kind::Swamp => flower::BLUE_ORCHID,
        _ => {
            if rand.next_int_bounded(3) > 0 {
                flower::DANDELION
            } else {
                flower::POPPY
            }
        }
    };
    flower_state(ordinal)
}

const TALL_GRASS: u16 = blocks::state(id::TALLGRASS, 1);
const FERN: u16 = blocks::state(id::TALLGRASS, 2);

/// `Biome.getRandomWorldGenForGrass` and its overrides, as the `WorldGenTallGrass` state.
pub fn grass(b: &Biome, rand: &mut JavaRandom) -> u16 {
    match b.kind {
        Kind::Jungle => {
            if rand.next_int_bounded(4) == 0 {
                FERN
            } else {
                TALL_GRASS
            }
        }
        Kind::Taiga(_) => {
            if rand.next_int_bounded(5) > 0 {
                FERN
            } else {
                TALL_GRASS
            }
        }
        _ => TALL_GRASS,
    }
}

/// The `DOUBLE_PLANT_GENERATOR` loops in `BiomePlains`, `BiomeSavanna` and `BiomeTaiga`.
fn double_plants(w: &mut PopWorld, rand: &mut JavaRandom, pos: Pos, variant: u16, n: i32) {
    for _ in 0..n {
        let x = rand.next_int_bounded(16) + 8;
        let z = rand.next_int_bounded(16) + 8;
        let y = rand.next_int_bounded(w.get_height(pos.x + x, pos.z + z) + 32);
        features::double_plant(w, rand, variant, pos.add(x, y, z));
    }
}

/// `Biome.decorate` and its overrides. `pos` is the chunk's block origin.
pub fn decorate(w: &mut PopWorld, rand: &mut JavaRandom, b: &'static Biome, pos: Pos) {
    let mut d = decorator_for(b);
    match b.kind {
        Kind::Desert => {
            decorator::decorate(w, rand, b, &d, pos);
            if rand.next_int_bounded(1000) == 0 {
                let x = rand.next_int_bounded(16) + 8;
                let z = rand.next_int_bounded(16) + 8;
                let p = w.height_pos(pos.add(x, 0, z)).up();
                features2::desert_wells(w, rand, p);
            }
            if rand.next_int_bounded(64) == 0 {
                features2::fossils(w, rand, pos);
            }
        }
        Kind::Forest | Kind::ForestMutated => {
            let t = forest_type(b);
            if t == ForestType::Roofed {
                add_mushrooms(w, rand, b, pos);
            }
            let mut i = rand.next_int_bounded(5) - 3;
            if t == ForestType::Flower {
                i += 2;
            }
            add_double_plants(w, rand, pos, i);
            decorator::decorate(w, rand, b, &d, pos);
        }
        Kind::Hills(_) => {
            decorator::decorate(w, rand, b, &d, pos);
            let i = 3 + rand.next_int_bounded(6);
            for _ in 0..i {
                let k = rand.next_int_bounded(16);
                let l = rand.next_int_bounded(28) + 4;
                let i1 = rand.next_int_bounded(16);
                let bp = pos.add(k, l, i1);
                if blocks::block_of(w.get(bp)) == id::STONE {
                    w.set(bp, blocks::state(id::EMERALD_ORE, 0), 2);
                }
            }
            for _ in 0..7 {
                let k1 = rand.next_int_bounded(16);
                let l1 = rand.next_int_bounded(64);
                let i2 = rand.next_int_bounded(16);
                features::minable(w, rand, blocks::state(id::MONSTER_EGG, 0), 9, pos.add(k1, l1, i2));
            }
        }
        Kind::Jungle => {
            decorator::decorate(w, rand, b, &d, pos);
            let i = rand.next_int_bounded(16) + 8;
            let j = rand.next_int_bounded(16) + 8;
            let k = rand.next_int_bounded(w.get_height(pos.x + i, pos.z + j) * 2);
            features::melon(w, rand, pos.add(i, k, j));
            for _ in 0..50 {
                let k = rand.next_int_bounded(16) + 8;
                let i1 = rand.next_int_bounded(16) + 8;
                features::vines(w, rand, pos.add(k, 128, i1));
            }
        }
        Kind::Plains => {
            let d0 = GRASS_COLOR_NOISE.get_value((pos.x + 8) as f64 / 200.0, (pos.z + 8) as f64 / 200.0);
            if d0 < -0.8 {
                d.flowers = 15;
                d.grass = 5;
            } else {
                d.flowers = 4;
                d.grass = 10;
                double_plants(w, rand, pos, plant::GRASS, 7);
            }
            if plains_sunflowers(b) {
                double_plants(w, rand, pos, plant::SUNFLOWER, 10);
            }
            decorator::decorate(w, rand, b, &d, pos);
        }
        Kind::Savanna => {
            double_plants(w, rand, pos, plant::GRASS, 7);
            decorator::decorate(w, rand, b, &d, pos);
        }
        Kind::Snow { super_icy } => {
            if super_icy {
                for _ in 0..3 {
                    let j = rand.next_int_bounded(16) + 8;
                    let k = rand.next_int_bounded(16) + 8;
                    let p = w.height_pos(pos.add(j, 0, k));
                    features2::ice_spike(w, rand, p);
                }
                for _ in 0..2 {
                    let i1 = rand.next_int_bounded(16) + 8;
                    let j1 = rand.next_int_bounded(16) + 8;
                    let p = w.height_pos(pos.add(i1, 0, j1));
                    features::ice_path(w, rand, 4, p);
                }
            }
            decorator::decorate(w, rand, b, &d, pos);
        }
        Kind::Swamp => {
            decorator::decorate(w, rand, b, &d, pos);
            if rand.next_int_bounded(64) == 0 {
                features2::fossils(w, rand, pos);
            }
        }
        Kind::Taiga(t) => {
            if t == TaigaType::Mega || t == TaigaType::MegaSpruce {
                let i = rand.next_int_bounded(3);
                for _ in 0..i {
                    let k = rand.next_int_bounded(16) + 8;
                    let l = rand.next_int_bounded(16) + 8;
                    let p = w.height_pos(pos.add(k, 0, l));
                    features::block_blob(w, rand, id::MOSSY_COBBLESTONE, 0, p);
                }
            }
            double_plants(w, rand, pos, plant::FERN, 7);
            decorator::decorate(w, rand, b, &d, pos);
        }
        // SavannaMutated calls `decorator.decorate` directly, skipping the savanna grass.
        _ => decorator::decorate(w, rand, b, &d, pos),
    }
}

/// `BiomeForest.addMushrooms`
fn add_mushrooms(w: &mut PopWorld, rand: &mut JavaRandom, b: &'static Biome, pos: Pos) {
    for i in 0..4 {
        for j in 0..4 {
            let k = i * 4 + 1 + 8 + rand.next_int_bounded(3);
            let l = j * 4 + 1 + 8 + rand.next_int_bounded(3);
            let bp = w.height_pos(pos.add(k, 0, l));
            if rand.next_int_bounded(20) == 0 {
                features2::big_mushroom(w, rand, None, bp);
            } else {
                let tree = random_tree(b, rand);
                tree.set_decoration_defaults(w);
                if tree.generate(w, rand, bp) {
                    tree.generate_saplings(w, rand, bp);
                }
            }
        }
    }
}

/// `BiomeForest.addDoublePlants`
fn add_double_plants(w: &mut PopWorld, rand: &mut JavaRandom, pos: Pos, n: i32) {
    for _ in 0..n {
        let variant = match rand.next_int_bounded(3) {
            0 => plant::SYRINGA,
            1 => plant::ROSE,
            _ => plant::PAEONIA,
        };
        for _ in 0..5 {
            let l = rand.next_int_bounded(16) + 8;
            let i1 = rand.next_int_bounded(16) + 8;
            let j1 = rand.next_int_bounded(w.get_height(pos.x + l, pos.z + i1) + 32);
            if features::double_plant(w, rand, variant, Pos::new(pos.x + l, j1, pos.z + i1)) {
                break;
            }
        }
    }
}

/// One `Biome.SpawnListEntry` from the CREATURE list. Only the weights, group sizes and whether
/// the entity is a sheep (whose `onInitialSpawn` draws `World.rand`) matter to population.
#[derive(Clone, Copy, Debug)]
pub struct SpawnEntry {
    pub weight: i32,
    pub min: i32,
    pub max: i32,
    pub sheep: bool,
}

const fn entry(weight: i32, min: i32, max: i32) -> SpawnEntry {
    SpawnEntry { weight, min, max, sheep: false }
}

/// `Biome.getSpawnableList(CREATURE)` after each constructor.
pub fn creatures(b: &Biome) -> Vec<SpawnEntry> {
    // Biome(): Sheep, Pig, Chicken, Cow.
    let mut v = vec![SpawnEntry { weight: 12, min: 4, max: 4, sheep: true }, entry(10, 4, 4), entry(10, 4, 4), entry(8, 4, 4)];
    match b.kind {
        Kind::Beach | Kind::StoneBeach | Kind::Ocean | Kind::River | Kind::Mesa { .. } => v.clear(),
        Kind::Desert => {
            v.clear();
            v.push(entry(4, 2, 3)); // Rabbit
        }
        Kind::Forest | Kind::ForestMutated => match forest_type(b) {
            ForestType::Flower => v.push(entry(4, 2, 3)), // Rabbit
            ForestType::Normal => v.push(entry(5, 4, 4)), // Wolf
            _ => {}
        },
        Kind::Hills(_) => v.push(entry(5, 4, 6)), // Llama
        Kind::Jungle => {
            v.push(entry(40, 1, 2)); // Parrot
            v.push(entry(10, 4, 4)); // Chicken
        }
        Kind::MushroomIsland => {
            v.clear();
            v.push(entry(8, 4, 8)); // Mooshroom
        }
        Kind::Plains => {
            v.push(entry(5, 2, 6)); // Horse
            v.push(entry(1, 1, 3)); // Donkey
        }
        Kind::Savanna | Kind::SavannaMutated => {
            v.push(entry(1, 2, 6)); // Horse
            v.push(entry(1, 1, 1)); // Donkey
            if b.base_height > 1.1 {
                v.push(entry(8, 4, 4)); // Llama
            }
        }
        Kind::Snow { .. } => {
            v.clear();
            v.push(entry(10, 2, 3)); // Rabbit
            v.push(entry(1, 1, 2)); // PolarBear
        }
        Kind::Taiga(_) => {
            v.push(entry(8, 4, 4)); // Wolf
            v.push(entry(4, 2, 3)); // Rabbit
        }
        _ => {}
    }
    v
}

/// `Biome.getSpawningChance`
pub fn spawning_chance(b: &Biome) -> f32 {
    match b.kind {
        Kind::Snow { .. } => 0.07,
        _ => 0.1,
    }
}
