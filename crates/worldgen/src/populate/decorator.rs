//! `BiomeDecorator`: the per-biome counts and `genDecorations`, the pass every biome's `decorate`
//! ends in. The biome-specific picks (tree, flower, grass) come from [`super::biomes`].

use super::biomes;
use super::features;
use super::features2;
use super::state::Pos;
use super::world::PopWorld;
use crate::biome::Biome;
use crate::blocks::{self, id};
use crate::java_random::JavaRandom;

/// The `BiomeDecorator` fields a biome constructor sets, with the field initialisers as defaults.
#[derive(Clone, Copy, Debug)]
pub struct Decorator {
    pub waterlily: i32,
    pub trees: i32,
    pub extra_tree_chance: f32,
    pub flowers: i32,
    pub grass: i32,
    pub dead_bush: i32,
    pub mushrooms: i32,
    pub reeds: i32,
    pub cacti: i32,
    pub gravel_patches: i32,
    pub sand_patches: i32,
    pub clay: i32,
    pub big_mushrooms: i32,
    pub generate_falls: bool,
    /// `BiomeMesa.Decorator.generateOres`: the extra gold pass.
    pub mesa_gold: bool,
}

impl Default for Decorator {
    fn default() -> Self {
        Decorator {
            waterlily: 0,
            trees: 0,
            extra_tree_chance: 0.1,
            flowers: 2,
            grass: 1,
            dead_bush: 0,
            mushrooms: 0,
            reeds: 0,
            cacti: 0,
            gravel_patches: 1,
            sand_patches: 3,
            clay: 1,
            big_mushrooms: 0,
            generate_falls: true,
            mesa_gold: false,
        }
    }
}

/// One `WorldGenMinable` the decorator builds in `decorate`, sized from the default
/// `ChunkGeneratorSettings`, with its count and height range.
struct Ore {
    state: u16,
    size: i32,
    count: i32,
    min: i32,
    max: i32,
}

/// `generateOres` order: dirt, gravel, diorite, granite, andesite, coal, iron, gold, redstone,
/// diamond (all `genStandardOre1`), then lapis through `genStandardOre2`.
const ORES: [Ore; 10] = [
    Ore { state: blocks::state(id::DIRT, 0), size: 33, count: 10, min: 0, max: 256 },
    Ore { state: blocks::state(id::GRAVEL, 0), size: 33, count: 8, min: 0, max: 256 },
    Ore { state: blocks::state(id::STONE, 3), size: 33, count: 10, min: 0, max: 80 },
    Ore { state: blocks::state(id::STONE, 1), size: 33, count: 10, min: 0, max: 80 },
    Ore { state: blocks::state(id::STONE, 5), size: 33, count: 10, min: 0, max: 80 },
    Ore { state: blocks::state(id::COAL_ORE, 0), size: 17, count: 20, min: 0, max: 128 },
    Ore { state: blocks::state(id::IRON_ORE, 0), size: 9, count: 20, min: 0, max: 64 },
    Ore { state: blocks::state(id::GOLD_ORE, 0), size: 9, count: 2, min: 0, max: 32 },
    Ore { state: blocks::state(id::REDSTONE_ORE, 0), size: 8, count: 8, min: 0, max: 16 },
    Ore { state: blocks::state(id::DIAMOND_ORE, 0), size: 8, count: 1, min: 0, max: 16 },
];
const LAPIS: u16 = blocks::state(id::LAPIS_ORE, 0);
const LAPIS_SIZE: i32 = 7;
const LAPIS_COUNT: i32 = 1;
const LAPIS_CENTER: i32 = 16;
const LAPIS_SPREAD: i32 = 16;
const GOLD: usize = 7;

/// `BiomeDecorator.decorate`
// DROPPED: the `decorating` re-entry guard; population here never nests.
pub fn decorate(w: &mut PopWorld, rand: &mut JavaRandom, b: &'static Biome, d: &Decorator, chunk_pos: Pos) {
    gen_decorations(w, rand, b, d, chunk_pos);
}

/// `BiomeDecorator.genStandardOre1`
fn gen_standard_ore1(w: &mut PopWorld, rand: &mut JavaRandom, chunk_pos: Pos, count: i32, ore: u16, size: i32, mut min: i32, mut max: i32) {
    if max < min {
        std::mem::swap(&mut min, &mut max);
    } else if max == min {
        if min < 255 {
            max += 1;
        } else {
            min -= 1;
        }
    }
    for _ in 0..count {
        let x = rand.next_int_bounded(16);
        let y = rand.next_int_bounded(max - min) + min;
        let z = rand.next_int_bounded(16);
        features::minable(w, rand, ore, size, chunk_pos.add(x, y, z));
    }
}

/// `BiomeDecorator.genStandardOre2`
fn gen_standard_ore2(w: &mut PopWorld, rand: &mut JavaRandom, chunk_pos: Pos, count: i32, ore: u16, size: i32, center: i32, spread: i32) {
    for _ in 0..count {
        let x = rand.next_int_bounded(16);
        let y = rand.next_int_bounded(spread) + rand.next_int_bounded(spread) + center - spread;
        let z = rand.next_int_bounded(16);
        features::minable(w, rand, ore, size, chunk_pos.add(x, y, z));
    }
}

/// `BiomeDecorator.generateOres`, plus the `BiomeMesa.Decorator` override.
fn generate_ores(w: &mut PopWorld, rand: &mut JavaRandom, d: &Decorator, chunk_pos: Pos) {
    for o in &ORES {
        gen_standard_ore1(w, rand, chunk_pos, o.count, o.state, o.size, o.min, o.max);
    }
    gen_standard_ore2(w, rand, chunk_pos, LAPIS_COUNT, LAPIS, LAPIS_SIZE, LAPIS_CENTER, LAPIS_SPREAD);
    if d.mesa_gold {
        let g = &ORES[GOLD];
        gen_standard_ore1(w, rand, chunk_pos, 20, g.state, g.size, 32, 80);
    }
}

/// The `random.nextInt(16) + 8` pair most decorator loops start with.
fn offset(rand: &mut JavaRandom) -> (i32, i32) {
    let x = rand.next_int_bounded(16) + 8;
    let z = rand.next_int_bounded(16) + 8;
    (x, z)
}

/// The `getHeight(...).getY() * 2` guard: draws `nextInt(h * 2)` when positive.
fn height2(w: &mut PopWorld, rand: &mut JavaRandom, chunk_pos: Pos, x: i32, z: i32) -> Option<Pos> {
    let h = w.get_height(chunk_pos.x + x, chunk_pos.z + z) * 2;
    if h > 0 {
        let y = rand.next_int_bounded(h);
        Some(chunk_pos.add(x, y, z))
    } else {
        None
    }
}

/// `BiomeDecorator.genDecorations`
fn gen_decorations(w: &mut PopWorld, rand: &mut JavaRandom, b: &'static Biome, d: &Decorator, chunk_pos: Pos) {
    generate_ores(w, rand, d, chunk_pos);

    for _ in 0..d.sand_patches {
        let (x, z) = offset(rand);
        let p = w.top_solid_or_liquid(chunk_pos.add(x, 0, z));
        features::sand(w, rand, id::SAND, 7, p);
    }
    for _ in 0..d.clay {
        let (x, z) = offset(rand);
        let p = w.top_solid_or_liquid(chunk_pos.add(x, 0, z));
        features::clay(w, rand, 4, p);
    }
    for _ in 0..d.gravel_patches {
        let (x, z) = offset(rand);
        let p = w.top_solid_or_liquid(chunk_pos.add(x, 0, z));
        features::sand(w, rand, id::GRAVEL, 6, p);
    }

    let mut k1 = d.trees;
    if rand.next_float() < d.extra_tree_chance {
        k1 += 1;
    }
    for _ in 0..k1 {
        let (x, z) = offset(rand);
        let tree = biomes::random_tree(b, rand);
        tree.set_decoration_defaults(w);
        let p = w.height_pos(chunk_pos.add(x, 0, z));
        if tree.generate(w, rand, p) {
            tree.generate_saplings(w, rand, p);
        }
    }

    for _ in 0..d.big_mushrooms {
        let (x, z) = offset(rand);
        let p = w.height_pos(chunk_pos.add(x, 0, z));
        features2::big_mushroom(w, rand, None, p);
    }

    for _ in 0..d.flowers {
        let (x, z) = offset(rand);
        let j14 = w.get_height(chunk_pos.x + x, chunk_pos.z + z) + 32;
        if j14 > 0 {
            let y = rand.next_int_bounded(j14);
            let p = chunk_pos.add(x, y, z);
            let flower = biomes::pick_flower(b, rand, p);
            // `getDefaultState().getMaterial() != AIR` holds for both flower blocks.
            features::flowers(w, rand, flower, p);
        }
    }

    for _ in 0..d.grass {
        let (x, z) = offset(rand);
        if let Some(p) = height2(w, rand, chunk_pos, x, z) {
            let grass = biomes::grass(b, rand);
            features::tall_grass(w, rand, grass, p);
        }
    }

    for _ in 0..d.dead_bush {
        let (x, z) = offset(rand);
        if let Some(p) = height2(w, rand, chunk_pos, x, z) {
            features::dead_bush(w, rand, p);
        }
    }

    for _ in 0..d.waterlily {
        let (x, z) = offset(rand);
        if let Some(mut p) = height2(w, rand, chunk_pos, x, z) {
            while p.y > 0 {
                let below = p.down();
                if !w.is_air(below) {
                    break;
                }
                p = below;
            }
            features::waterlily(w, rand, p);
        }
    }

    for _ in 0..d.mushrooms {
        if rand.next_int_bounded(4) == 0 {
            let (x, z) = offset(rand);
            let p = w.height_pos(chunk_pos.add(x, 0, z));
            features::bush(w, rand, id::BROWN_MUSHROOM, p);
        }
        if rand.next_int_bounded(8) == 0 {
            let (x, z) = offset(rand);
            if let Some(p) = height2(w, rand, chunk_pos, x, z) {
                features::bush(w, rand, id::RED_MUSHROOM, p);
            }
        }
    }

    if rand.next_int_bounded(4) == 0 {
        let (x, z) = offset(rand);
        if let Some(p) = height2(w, rand, chunk_pos, x, z) {
            features::bush(w, rand, id::BROWN_MUSHROOM, p);
        }
    }
    if rand.next_int_bounded(8) == 0 {
        let (x, z) = offset(rand);
        if let Some(p) = height2(w, rand, chunk_pos, x, z) {
            features::bush(w, rand, id::RED_MUSHROOM, p);
        }
    }

    for _ in 0..d.reeds {
        let (x, z) = offset(rand);
        if let Some(p) = height2(w, rand, chunk_pos, x, z) {
            features::reed(w, rand, p);
        }
    }
    for _ in 0..10 {
        let (x, z) = offset(rand);
        if let Some(p) = height2(w, rand, chunk_pos, x, z) {
            features::reed(w, rand, p);
        }
    }

    if rand.next_int_bounded(32) == 0 {
        let (x, z) = offset(rand);
        if let Some(p) = height2(w, rand, chunk_pos, x, z) {
            features::pumpkin(w, rand, p);
        }
    }

    for _ in 0..d.cacti {
        let (x, z) = offset(rand);
        if let Some(p) = height2(w, rand, chunk_pos, x, z) {
            features::cactus(w, rand, p);
        }
    }

    if d.generate_falls {
        for _ in 0..50 {
            let (x, z) = offset(rand);
            let i17 = rand.next_int_bounded(248) + 8;
            if i17 > 0 {
                let y = rand.next_int_bounded(i17);
                features2::liquids(w, rand, id::FLOWING_WATER, chunk_pos.add(x, y, z));
            }
        }
        for _ in 0..20 {
            let (x, z) = offset(rand);
            let a = rand.next_int_bounded(240) + 8;
            let b2 = rand.next_int_bounded(a) + 8;
            let y = rand.next_int_bounded(b2);
            features2::liquids(w, rand, id::FLOWING_LAVA, chunk_pos.add(x, y, z));
        }
    }
}
