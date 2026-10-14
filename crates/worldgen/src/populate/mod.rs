//! A port of `ChunkGeneratorOverworld.populate`: lakes, dungeons, the biome decorator (ores,
//! disks, trees, plants, springs), animal spawning draws, and ice and snow.
//!
//! Structures (`MapGenStructure.generateStructure`) are not generated.

pub mod behavior;
pub mod biomes;
pub mod decorator;
pub mod features;
pub mod features2;
pub mod liquid;
pub mod packed;
pub mod props;
pub mod spawn;
pub mod state;
pub mod trees;
pub mod trees2;
pub mod trees3;
pub mod world;

pub use packed::PackedChunk;
pub use state::{Facing, Pos};
pub use world::{PopChunk, PopStatics, PopWorld};

use crate::blocks::{self, id};
use crate::java_random::JavaRandom;

/// `ChunkGeneratorSettings` defaults the population pass reads.
const WATER_LAKE_CHANCE: i32 = 4;
const LAVA_LAKE_CHANCE: i32 = 80;
const DUNGEON_CHANCE: i32 = 8;
const SEA_LEVEL: i32 = 63;

/// `ChunkGeneratorOverworld.populate(x, z)`. Every chunk the pass touches must already be in `w`
/// (or it is generated on access, as `provideChunk` would).
pub fn populate(w: &mut PopWorld, x: i32, z: i32) {
    w.fall_instantly = true;
    let i = x * 16;
    let j = z * 16;
    let mut blockpos = Pos::new(i, 0, j);
    let biome = w.biome(blockpos.add(16, 0, 16));
    let seed = w.gen.seed();
    let mut rand = JavaRandom::new(seed);
    let k = (rand.next_long() / 2).wrapping_mul(2).wrapping_add(1);
    let l = (rand.next_long() / 2).wrapping_mul(2).wrapping_add(1);
    rand.set_seed((x as i64).wrapping_mul(k).wrapping_add((z as i64).wrapping_mul(l)) ^ seed);
    // DROPPED: mineshaft, village, stronghold, temple, monument and mansion generateStructure.
    // The village result `flag` stays false.
    let flag = false;

    if biome.id != 2 && biome.id != 17 && !flag && rand.next_int_bounded(WATER_LAKE_CHANCE) == 0 {
        let i1 = rand.next_int_bounded(16) + 8;
        let j1 = rand.next_int_bounded(256);
        let k1 = rand.next_int_bounded(16) + 8;
        features2::lakes(w, &mut rand, id::WATER, blockpos.add(i1, j1, k1));
    }

    if !flag && rand.next_int_bounded(LAVA_LAKE_CHANCE / 10) == 0 {
        let i2 = rand.next_int_bounded(16) + 8;
        let a = rand.next_int_bounded(248) + 8;
        let l2 = rand.next_int_bounded(a);
        let k3 = rand.next_int_bounded(16) + 8;
        if l2 < SEA_LEVEL || rand.next_int_bounded(LAVA_LAKE_CHANCE / 8) == 0 {
            features2::lakes(w, &mut rand, id::LAVA, blockpos.add(i2, l2, k3));
        }
    }

    for _ in 0..DUNGEON_CHANCE {
        let i3 = rand.next_int_bounded(16) + 8;
        let l3 = rand.next_int_bounded(256);
        let l1 = rand.next_int_bounded(16) + 8;
        features2::dungeons(w, &mut rand, blockpos.add(i3, l3, l1));
    }

    biomes::decorate(w, &mut rand, biome, Pos::new(i, 0, j));
    spawn::perform_world_gen_spawning(w, biome, i + 8, j + 8, &mut rand);
    blockpos = blockpos.add(8, 0, 8);

    for k2 in 0..16 {
        for j3 in 0..16 {
            let p1 = w.precipitation_height(blockpos.add(k2, 0, j3));
            let p2 = p1.down();
            if w.can_block_freeze(p2, false) {
                w.set(p2, blocks::ICE, 2);
            }
            if w.can_snow_at(p1) {
                w.set(p1, blocks::SNOW_LAYER, 2);
            }
        }
    }

    w.fall_instantly = false;
}
