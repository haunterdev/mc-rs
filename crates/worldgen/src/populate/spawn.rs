//! `WorldEntitySpawner.performWorldGenSpawning`: the animal groups a chunk starts with. The
//! entities themselves are not kept; the pass runs for the random draws it makes, which every
//! later chunk's population depends on.

use super::biomes::{self, SpawnEntry};
use super::state::{self, Pos};
use super::world::PopWorld;
use crate::biome::Biome;
use crate::blocks::{self, id};
use crate::java_random::JavaRandom;

/// `WeightedRandom.getRandomItem(random, list)`
fn weighted_pick(rand: &mut JavaRandom, list: &[SpawnEntry]) -> SpawnEntry {
    let total: i32 = list.iter().map(|e| e.weight).sum();
    let mut weight = rand.next_int_bounded(total);
    for e in list {
        weight -= e.weight;
        if weight < 0 {
            return *e;
        }
    }
    unreachable!("weights sum to the bound")
}

/// `WorldEntitySpawner.isValidEmptySpawnBlock`
// DROPPED: the `canProvidePower` and rail tests; population places neither kind of block.
fn is_valid_empty_spawn_block(s: u16) -> bool {
    !state::is_block_normal_cube(s) && !state::mat_liquid(s)
}

/// `WorldEntitySpawner.canCreatureTypeSpawnAtLocation(ON_GROUND, world, pos)`
// DROPPED: the world border test; the default border holds every coordinate population reaches.
fn can_spawn_on_ground(w: &mut PopWorld, pos: Pos) -> bool {
    let s = w.get(pos);
    let below = w.get(pos.down());
    if !state::is_top_solid(below) {
        return false;
    }
    let block = blocks::block_of(below);
    let flag = block != id::BEDROCK && block != id::BARRIER;
    flag && is_valid_empty_spawn_block(s) && is_valid_empty_spawn_block(w.get(pos.up()))
}

/// `EntitySheep.getRandomSheepColor(world.rand)`, the one creature `onInitialSpawn` that draws
/// from `World.rand`.
fn sheep_color_draws(w: &mut PopWorld) {
    let i = w.rand.next_int_bounded(100);
    if i >= 18 {
        w.rand.next_int_bounded(500);
    }
}

/// `WorldEntitySpawner.performWorldGenSpawning(world, biome, centerX, centerZ, 16, 16, random)`
pub fn perform_world_gen_spawning(w: &mut PopWorld, b: &Biome, center_x: i32, center_z: i32, rand: &mut JavaRandom) {
    const DIAMETER: i32 = 16;
    let list = biomes::creatures(b);
    if list.is_empty() {
        return;
    }
    let chance = biomes::spawning_chance(b);
    while rand.next_float() < chance {
        let entry = weighted_pick(&mut w.rand, &list);
        let i = entry.min + rand.next_int_bounded(1 + entry.max - entry.min);
        let mut j = center_x + rand.next_int_bounded(DIAMETER);
        let mut k = center_z + rand.next_int_bounded(DIAMETER);
        let l = j;
        let i1 = k;
        for _ in 0..i {
            let mut flag = false;
            let mut k1 = 0;
            while !flag && k1 < 4 {
                let bp = w.top_solid_or_liquid(Pos::new(j, 0, k));
                if can_spawn_on_ground(w, bp) {
                    // setLocationAndAngles yaw
                    rand.next_float();
                    // DROPPED: the entity, spawnEntity and the rest of onInitialSpawn, which
                    // draw only from the entity's own Random.
                    if entry.sheep {
                        sheep_color_draws(w);
                    }
                    flag = true;
                }
                j += rand.next_int_bounded(5) - rand.next_int_bounded(5);
                k += rand.next_int_bounded(5) - rand.next_int_bounded(5);
                // Vanilla bounds k with `centerZ + diameterX`; both diameters are 16.
                while j < center_x || j >= center_x + DIAMETER || k < center_z || k >= center_z + DIAMETER {
                    j = l + rand.next_int_bounded(5) - rand.next_int_bounded(5);
                    k = i1 + rand.next_int_bounded(5) - rand.next_int_bounded(5);
                }
                k1 += 1;
            }
        }
    }
}
