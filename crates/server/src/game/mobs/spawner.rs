//! Natural spawning and despawning (T9d): `WorldServer.tick`'s call into
//! `WorldEntitySpawner.findChunksForSpawning`, `canCreatureTypeSpawnAtLocation`, the per-class
//! `getCanSpawnHere` (`EntityLiving`, `EntityCreature`, `EntityMob`, `EntityAnimal`),
//! `performWorldGenSpawning` for chunks that were just populated, and `EntityLiving.despawnEntity`.
//! The per-biome lists are `spawn_lists.rs`.
//!
//! DROPPED across the file: `onInitialSpawn` (a zombie's baby roll and equipment, a skeleton's
//! gear, a sheep's colour: they draw only from the entity's own `Random`), the world border (the
//! default holds every coordinate), `spawn-monsters` and `spawn-animals` in server.properties
//! (both on), `DifficultyInstance` (regional difficulty feeds only `onInitialSpawn`), the
//! structure spawn lists, and chicken jockeys (so a chicken never despawns). Java's `HashSet`
//! iteration order of `eligibleChunksForSpawning` and `Math.random()` cannot match a run of the
//! original anyway, so the set keeps insertion order and `Math.random()` is a generator of its own.

use std::collections::HashSet;

use sim::collision::{contains_any_liquid, floor, get_collision_boxes, Aabb};
use sim::region::{props, Mat};
use worldgen::biome::{Biome, Kind};
use worldgen::java_random::JavaRandom;
use worldgen::populate::state as ws;

use super::spawn_lists::{self as lists, CreatureType, Entry, Placement};
use super::{def_by_name, spawn_initial, Mob, MobDef};
use crate::game::food::Difficulty;
use crate::game::terrain::{self, Blocks};
use crate::game::{players, Game};

/// `WorldEntitySpawner.MOB_COUNT_DIV`, `(int)Math.pow(17, 2)`.
pub const MOB_COUNT_DIV: i32 = 289;
/// `isAnyPlayerWithinRangeAt(..., 24)`, squared; and the spawn point's `>= 576`.
pub const PLAYER_RANGE_SQ: f64 = 576.0;
/// `EntityLiving.despawnEntity`: beyond 128 blocks (squared), beyond 32 (squared), and the idle
/// ticks the random despawn waits for.
pub const DESPAWN_FAR_SQ: f64 = 16384.0;
pub const DESPAWN_NEAR_SQ: f64 = 1024.0;
pub const DESPAWN_IDLE: i32 = 600;

/// What the spawner keeps between ticks.
pub struct State {
    /// `World.rand`
    pub world_rand: JavaRandom,
    /// `Math.random()`
    pub math_random: JavaRandom,
    /// Chunks whose `populate` already ran `performWorldGenSpawning`, saved with the mobs so a chunk
    /// the terrain generates again in the next session does not get its animals twice.
    pub worldgen_done: HashSet<(i32, i32)>,
}

impl Default for State {
    fn default() -> State {
        State {
            world_rand: JavaRandom::new(crate::config::random_u64() as i64),
            math_random: JavaRandom::new(crate::config::random_u64() as i64),
            worldgen_done: HashSet::new(),
        }
    }
}

/// One spawn entry's identity: `list.contains(entry)` is object identity in Java, and every biome
/// builds its own entries.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Picked {
    biome: u8,
    index: usize,
    entry: Entry,
}

/// What a tick of spawning reads about the world.
pub(crate) struct Ctx {
    /// The players that are not spectators: `playerEntities` through `NOT_SPECTATING`.
    pub players: Vec<[f64; 3]>,
    pub spawn_point: [i32; 3],
    pub difficulty: Difficulty,
    pub sky_subtracted: i32,
    pub thundering: bool,
}

/// The non-spectator players' feet, by session id.
pub fn player_positions(game: &Game) -> Vec<[f64; 3]> {
    let mut ids: Vec<u32> = game.sessions.keys().copied().collect();
    ids.sort_unstable();
    ids.into_iter()
        .filter_map(|id| {
            let s = &game.sessions[&id];
            (s.mode != proto::GM_SPECTATOR).then_some(s.state?.pos)
        })
        .collect()
}

/// `WorldProvider.calculateCelestialAngle(time, 1.0)`
fn celestial_angle(time: i64) -> f32 {
    let i = (time % 24000) as i32;
    let mut f = (i as f32 + 1.0) / 24000.0 - 0.25;
    if f < 0.0 {
        f += 1.0;
    }
    if f > 1.0 {
        f -= 1.0;
    }
    let f1 = 1.0 - ((f as f64 * std::f64::consts::PI).cos() + 1.0) as f32 / 2.0;
    f + (f1 - f) / 3.0
}

/// `World.calculateSkylightSubtracted(1.0)`. DROPPED: the rain and thunder strengths ramp over
/// time in vanilla; here they are 1 while the weather is on (`/weather`) and 0 otherwise.
pub fn skylight_subtracted(time: i64, raining: bool, thundering: bool) -> i32 {
    let f = celestial_angle(time);
    let mut f1 = 1.0 - (worldgen::math_helper::cos(f * 6.2831855) * 2.0 + 0.5);
    f1 = f1.clamp(0.0, 1.0);
    f1 = 1.0 - f1;
    let rain = if raining || thundering { 1.0f32 } else { 0.0 };
    f1 = (f1 as f64 * (1.0 - (rain * 5.0) as f64 / 16.0)) as f32;
    let thunder = if thundering { 1.0f32 } else { 0.0 };
    f1 = (f1 as f64 * (1.0 - (thunder * 5.0) as f64 / 16.0)) as f32;
    f1 = 1.0 - f1;
    (f1 * 11.0) as i32
}

/// `World.skylightSubtracted` now.
pub(crate) fn sky_subtracted(game: &Game) -> i32 {
    match terrain::world(game) {
        Some(w) => {
            let w = w.lock().unwrap();
            skylight_subtracted(w.time(), w.weather.raining, w.weather.thundering)
        }
        None => skylight_subtracted(0, false, false),
    }
}

pub(crate) fn context(game: &Game) -> Ctx {
    let (spawn_point, time, raining, thundering) = match terrain::world(game) {
        Some(w) => {
            let w = w.lock().unwrap();
            (w.spawn, w.time(), w.weather.raining, w.weather.thundering)
        }
        None => ([0; 3], 0, false, false),
    };
    Ctx {
        players: player_positions(game),
        spawn_point,
        difficulty: players::difficulty(game),
        sky_subtracted: skylight_subtracted(time, raining, thundering),
        thundering,
    }
}

// ---- block and light reads ----

fn state_at(game: &Game, x: i32, y: i32, z: i32) -> u16 {
    terrain::state(game, (x, y, z))
}

/// `(sky, block)` light as the chunk stores it; `None` while the chunk's neighbours are not final.
fn raw_light(game: &mut Game, x: i32, y: i32, z: i32) -> Option<(i32, i32)> {
    if y < 0 {
        return Some((0, 0));
    }
    terrain::light(game, (x, y.min(255), z)).map(|(s, b)| (s as i32, b as i32))
}

/// `World.getLight(pos)`: `Chunk.getLightSubtracted(pos, 0)`.
fn light_plain(game: &mut Game, x: i32, y: i32, z: i32) -> Option<i32> {
    raw_light(game, x, y, z).map(|(s, b)| s.max(b))
}

/// `World.getLight(pos, false)` with `skylightSubtracted` of `sub`.
fn light_sub(game: &mut Game, x: i32, y: i32, z: i32, sub: i32) -> Option<i32> {
    raw_light(game, x, y, z).map(|(s, b)| (s - sub).max(b))
}

/// `World.getLightFromNeighbors(pos)` = `getLight(pos, true)`.
pub(crate) fn light_from_neighbors(game: &mut Game, x: i32, y: i32, z: i32, sub: i32) -> Option<i32> {
    if (0..256).contains(&y) && props(state_at(game, x, y, z)).use_neighbor_brightness() {
        let mut best = light_sub(game, x, y + 1, z, sub)?;
        for (dx, dz) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
            best = best.max(light_sub(game, x + dx, y, z + dz, sub)?);
        }
        return Some(best);
    }
    light_sub(game, x, y, z, sub)
}

/// `World.getLightBrightness(pos)`: `WorldProvider.generateLightBrightnessTable` at that light.
pub(crate) fn brightness(light: i32) -> f32 {
    let f1 = 1.0 - light as f32 / 15.0;
    (1.0 - f1) / (f1 * 3.0 + 1.0)
}

/// `EntityMob.isValidLightLevel` given the sky light at the feet, and the light from neighbours
/// (already taken at the thunder-adjusted sky when it thunders).
pub fn valid_light_level(sky: i32, light: i32, rand: &mut JavaRandom) -> bool {
    if sky > rand.next_int_bounded(32) {
        false
    } else {
        light <= rand.next_int_bounded(8)
    }
}

// ---- placement ----

fn is_rail(s: u16) -> bool {
    matches!(s >> 4, 27 | 28 | 66 | 157)
}

/// `Block.canProvidePower(state)` of the 1.12.2 blocks that return true.
fn can_provide_power(s: u16) -> bool {
    match s >> 4 {
        // redstone wire, torches, lever, buttons, pressure plates, tripwire hook, repeaters,
        // comparators, detector rail, daylight detectors, redstone block, observer
        55 | 75 | 76 | 69 | 77 | 143 | 70 | 72 | 147 | 148 | 131 | 93 | 94 | 149 | 150 | 28 | 151 | 178 | 152 | 218 => true,
        // BlockChest: only the trapped chest
        146 => true,
        _ => false,
    }
}

/// `WorldEntitySpawner.isValidEmptySpawnBlock`
pub fn is_valid_empty_spawn_block(s: u16) -> bool {
    !ws::is_block_normal_cube(s) && !can_provide_power(s) && !ws::mat_liquid(s) && !is_rail(s)
}

fn is_water(s: u16) -> bool {
    props(s).mat == Mat::Water
}

/// `WorldEntitySpawner.canCreatureTypeSpawnAtLocation`. Unloaded chunks read as air, so a spot
/// beside the loaded area fails where vanilla would have loaded the chunk.
pub fn can_creature_type_spawn_at_location(game: &Game, placement: Placement, x: i32, y: i32, z: i32) -> bool {
    let s = state_at(game, x, y, z);
    if placement == Placement::InWater {
        return is_water(s) && is_water(state_at(game, x, y - 1, z)) && !ws::is_normal_cube(state_at(game, x, y + 1, z));
    }
    let below = state_at(game, x, y - 1, z);
    if !ws::is_top_solid(below) {
        return false;
    }
    let block = below >> 4;
    block != 7 && block != 166 && is_valid_empty_spawn_block(s) && is_valid_empty_spawn_block(state_at(game, x, y + 1, z))
}

/// `World.isAnyPlayerWithinRangeAt(x, y, z, 24)` and the spawn point's distance, as the pack loop
/// tests them: true when the spot is far enough from both.
pub fn far_enough(players: &[[f64; 3]], spawn_point: [i32; 3], x: f64, y: f64, z: f64) -> bool {
    let near = players.iter().any(|p| {
        let (dx, dy, dz) = (p[0] - x, p[1] - y, p[2] - z);
        dx * dx + dy * dy + dz * dz < PLAYER_RANGE_SQ
    });
    // BlockPos.distanceSq(x, y, z) is from the corner, not the centre.
    let (dx, dy, dz) = (spawn_point[0] as f64 - x, spawn_point[1] as f64 - y, spawn_point[2] as f64 - z);
    !near && dx * dx + dy * dy + dz * dz >= PLAYER_RANGE_SQ
}

/// `EntityLiving.isNotColliding`: no liquid, no block box, and no other living entity in the box
/// (`checkNoEntityCollision`: `preventEntitySpawning` is set on every `EntityLivingBase`).
fn is_not_colliding(game: &Game, ctx: &Ctx, bb: &Aabb) -> bool {
    let blocks = Blocks(game);
    if contains_any_liquid(&blocks, bb) || !get_collision_boxes(&blocks, bb).is_empty() {
        return false;
    }
    let hits_mob = game.mobs.mobs.iter().any(|m| !m.dead && m.body.bb.intersects(bb));
    let hits_player = ctx.players.iter().any(|p| Aabb::new(p[0] - 0.3, p[1], p[2] - 0.3, p[0] + 0.3, p[1] + 1.8, p[2] + 0.3).intersects(bb));
    !hits_mob && !hits_player
}

/// `EntityLiving.getCanSpawnHere` and the overrides above it for the kinds `MOBS` has.
/// `Block.canEntitySpawn` is true for every block, so the `EntityLiving` part always holds.
fn can_spawn_here(game: &mut Game, ctx: &Ctx, m: &Mob) -> bool {
    let (px, py, pz) = (m.body.pos.x, m.body.bb.min_y, m.body.pos.z);
    let (x, y, z) = (floor(px), floor(py), floor(pz));
    if lists::is_hostile(m.def) {
        // EntityMob: difficulty, isValidLightLevel (the entity's own Random), then EntityCreature's
        // getBlockPathWeight >= 0, which for EntityMob is 0.5 - getLightBrightness(pos).
        if ctx.difficulty == Difficulty::Peaceful {
            return false;
        }
        let Some((sky, _)) = raw_light(game, x, y, z) else { return false };
        let sub = if ctx.thundering { 10 } else { ctx.sky_subtracted };
        let Some(light) = light_from_neighbors(game, x, y, z, sub) else { return false };
        if !valid_light_level(sky, light, &mut game.mobs.rand) {
            return false;
        }
        let Some(plain) = light_from_neighbors(game, x, y, z, ctx.sky_subtracted) else { return false };
        0.5 - brightness(plain) >= 0.0
    } else {
        // EntityAnimal: grass underfoot, World.getLight(pos) > 8, then EntityCreature with
        // getBlockPathWeight 10 on grass.
        if state_at(game, x, y - 1, z) >> 4 != 2 {
            return false;
        }
        light_plain(game, x, y, z).is_some_and(|l| l > 8)
    }
}

/// `EntityLiving.getMaxSpawnedInChunk`. The kinds `MOBS` has all use the default.
pub fn max_spawned_in_chunk(_def: &MobDef) -> i32 {
    4
}

// ---- findChunksForSpawning ----

/// The chunks around the players that spawning reads, and the count `i` the caps scale by.
pub struct Eligible {
    pub chunks: Vec<(i32, i32)>,
    pub counted: i32,
}

/// The first half of `findChunksForSpawning`: every chunk within 8 of a non-spectator player is
/// counted once; those not on the edge and held and final (`isSentToPlayers`) are eligible.
pub fn eligible_chunks(game: &Game, players: &[[f64; 3]]) -> Eligible {
    let mut set: HashSet<(i32, i32)> = HashSet::new();
    let mut chunks = Vec::new();
    let mut counted = 0;
    for p in players {
        let (j, k) = (floor(p[0] / 16.0), floor(p[2] / 16.0));
        for i1 in -8..=8 {
            for j1 in -8..=8 {
                let edge = i1 == -8 || i1 == 8 || j1 == -8 || j1 == 8;
                let pos = (i1 + j, j1 + k);
                if !set.contains(&pos) {
                    counted += 1;
                    // DROPPED: worldBorder.contains(chunkpos).
                    if !edge && terrain::is_final(game, pos) {
                        set.insert(pos);
                        chunks.push(pos);
                    }
                }
            }
        }
    }
    Eligible { chunks, counted }
}

/// `enumcreaturetype.getMaxNumberOfCreature() * i / MOB_COUNT_DIV`
pub fn cap(t: CreatureType, counted: i32) -> i32 {
    t.max_number() * counted / MOB_COUNT_DIV
}

/// `World.countEntities(type.getCreatureClass())`: the mobs of the class that persistence does
/// not exempt.
pub fn count_entities(game: &Game, t: CreatureType) -> i32 {
    game.mobs.mobs.iter().filter(|m| !m.persistent && lists::creature_type_of(m.def) == t).count() as i32
}

/// `WorldServer.tick`'s mob spawner, then the chunks that were just populated.
pub fn step(game: &mut Game) {
    // performWorldGenSpawning runs inside a chunk's populate and does not read doMobSpawning.
    world_gen_step(game);
    // WorldServer.tick: `getGameRules().getBoolean("doMobSpawning")`.
    if !terrain::rule(game, "doMobSpawning", true) {
        return;
    }
    // DROPPED: spawnHostileMobs/spawnPeacefulMobs setAllowedSpawnTypes (both on), and the
    // DEBUG_ALL_BLOCK_STATES world type.
    let on_set_tick = game.ticks % 400 == 0;
    find_chunks_for_spawning(game, true, true, on_set_tick);
}

/// `WorldEntitySpawner.findChunksForSpawning(world, hostile, peaceful, spawnOnSetTickRate)`.
/// `on_set_tick` is `worldTotalTime % 400 == 0`; DROPPED: the game counts its own ticks, since the
/// world clock is kept in wall time.
pub fn find_chunks_for_spawning(game: &mut Game, hostile: bool, peaceful: bool, on_set_tick: bool) -> i32 {
    if !hostile && !peaceful {
        return 0;
    }
    let ctx = context(game);
    let eligible = eligible_chunks(game, &ctx.players);
    let mut st = std::mem::take(&mut game.mobs.spawner);
    let mut j4 = 0;
    for t in lists::TYPES {
        if (!t.peaceful() || peaceful) && (t.peaceful() || hostile) && (!t.animal() || on_set_tick) {
            let k4 = count_entities(game, t);
            let l4 = cap(t, eligible.counted);
            if k4 <= l4 {
                for &(cx, cz) in &eligible.chunks {
                    let Some(start) = random_chunk_position(game, &mut st, cx, cz) else { continue };
                    if ws::is_normal_cube(state_at(game, start.0, start.1, start.2)) {
                        continue;
                    }
                    if let Some(add) = pack_at(game, &ctx, &mut st, t, start) {
                        j4 += add;
                    }
                }
            }
        }
    }
    game.mobs.spawner = st;
    j4
}

/// `WorldEntitySpawner.getRandomChunkPosition`
fn random_chunk_position(game: &Game, st: &mut State, cx: i32, cz: i32) -> Option<(i32, i32, i32)> {
    let i = cx * 16 + st.world_rand.next_int_bounded(16);
    let j = cz * 16 + st.world_rand.next_int_bounded(16);
    let (height, top) = terrain::column(game, i, j)?;
    let k = round_up(height + 1, 16);
    let l = st.world_rand.next_int_bounded(if k > 0 { k } else { top + 16 - 1 });
    Some((i, l, j))
}

/// `MathHelper.roundUp(number, 16)` for a positive number.
fn round_up(number: i32, interval: i32) -> i32 {
    let i = number % interval;
    if i == 0 {
        number
    } else {
        number + interval - i
    }
}

/// The body of the chunk loop for one starting position (the part after `!isNormalCube`).
/// `None` stands for `continue label134`, the chunk being full; otherwise the amount added to the
/// return value.
pub(crate) fn pack_at(game: &mut Game, ctx: &Ctx, st: &mut State, t: CreatureType, start: (i32, i32, i32)) -> Option<i32> {
    let mut j2 = 0;
    let mut j4 = 0;
    for _k2 in 0..3 {
        let (mut l2, mut i3, mut j3) = start;
        let mut picked: Option<Picked> = None;
        let l3 = (st.math_random.next_double() * 4.0).ceil() as i32;
        for _i4 in 0..l3 {
            l2 += st.world_rand.next_int_bounded(6) - st.world_rand.next_int_bounded(6);
            i3 += st.world_rand.next_int_bounded(1) - st.world_rand.next_int_bounded(1);
            j3 += st.world_rand.next_int_bounded(6) - st.world_rand.next_int_bounded(6);
            let f = (l2 as f32 + 0.5) as f64;
            let f1 = (j3 as f32 + 0.5) as f64;
            if !far_enough(&ctx.players, ctx.spawn_point, f, i3 as f64, f1) {
                continue;
            }
            // getSpawnListEntryForTypeAt, once per pack, then canCreatureTypeSpawnHere on the
            // biome of every spot.
            let Some(biome) = terrain::biome(game, l2, j3) else { continue };
            let list = lists::spawnable_list(biome, t);
            if picked.is_none() {
                if list.is_empty() {
                    break;
                }
                let index = lists::pick(&mut st.world_rand, &list);
                picked = Some(Picked { biome: biome.id, index, entry: list[index] });
            }
            let p = picked.expect("picked above");
            let here = biome.id == p.biome && list.get(p.index) == Some(&p.entry);
            if !here || !can_creature_type_spawn_at_location(game, lists::placement(p.entry.name), l2, i3, j3) {
                continue;
            }
            // DROPPED: a kind `MOBS` lacks (slime, enderman, witch, squid, bat ...) is picked and
            // placed as above, then nothing is created.
            let yaw = st.world_rand.next_float() * 360.0;
            if let Some(def) = def_by_name(p.entry.name) {
                let cand = Mob::new(0, def, [f, i3 as f64, f1], yaw);
                if can_spawn_here(game, ctx, &cand) && is_not_colliding(game, ctx, &cand.body.bb) {
                    // DROPPED: the second isNotColliding that follows onInitialSpawn.
                    spawn_initial(game, def, [f, i3 as f64, f1], yaw);
                    j2 += 1;
                    if j2 >= max_spawned_in_chunk(def) {
                        return None;
                    }
                }
            }
            j4 += j2;
        }
    }
    Some(j4)
}

// ---- performWorldGenSpawning ----

/// `Biome.getSpawningChance`
pub fn spawning_chance(b: &Biome) -> f32 {
    match b.kind {
        Kind::Snow { .. } => 0.07,
        _ => 0.1,
    }
}

/// `ChunkGeneratorOverworld.populate`'s seeding of its `Random` for chunk `(x, z)`.
fn populate_random(seed: i64, x: i32, z: i32) -> JavaRandom {
    let mut rand = JavaRandom::new(seed);
    let k = (rand.next_long() / 2).wrapping_mul(2).wrapping_add(1);
    let l = (rand.next_long() / 2).wrapping_mul(2).wrapping_add(1);
    rand.set_seed((x as i64).wrapping_mul(k).wrapping_add((z as i64).wrapping_mul(l)) ^ seed);
    rand
}

/// Runs `performWorldGenSpawning` for every populated chunk near a player that has not had it.
/// DROPPED: vanilla runs it inside `populate`, continuing the generator that placed the
/// decorations; population here happens in worldgen, so the pass starts from the populate seed
/// instead, with `World.rand` for the weighted pick as before.
pub fn world_gen_step(game: &mut Game) {
    let reach = game.cfg.sim_distance.max(0) + 2;
    let mut centers: Vec<(i32, i32)> = player_positions(game).iter().map(|p| (floor(p[0]) >> 4, floor(p[2]) >> 4)).collect();
    centers.sort_unstable();
    centers.dedup();
    for c in centers {
        for x in c.0 - reach..=c.0 + reach {
            for z in c.1 - reach..=c.1 + reach {
                if game.mobs.spawner.worldgen_done.contains(&(x, z)) || !terrain::is_populated(game, (x, z)) {
                    continue;
                }
                // populate(x, z) works on the 2x2 chunks from (x, z) and reads the biome at +16.
                let Some(biome) = terrain::biome(game, x * 16 + 16, z * 16 + 16) else { continue };
                if terrain::biome(game, x * 16 + 8, z * 16 + 8).is_none() || terrain::biome(game, x * 16 + 24, z * 16 + 24).is_none() {
                    continue;
                }
                game.mobs.spawner.worldgen_done.insert((x, z));
                let mut rand = populate_random(game.cfg.seed, x, z);
                perform_world_gen_spawning(game, biome, x * 16 + 8, z * 16 + 8, 16, 16, &mut rand);
            }
        }
    }
}

/// `WorldEntitySpawner.performWorldGenSpawning(world, biome, centerX, centerZ, diameterX,
/// diameterZ, random)`.
pub fn perform_world_gen_spawning(game: &mut Game, biome: &Biome, center_x: i32, center_z: i32, diameter_x: i32, diameter_z: i32, rand: &mut JavaRandom) {
    let list = lists::spawnable_list(biome, CreatureType::Creature);
    if list.is_empty() {
        return;
    }
    let chance = spawning_chance(biome);
    while rand.next_float() < chance {
        let entry = list[lists::pick(&mut game.mobs.spawner.world_rand, &list)];
        let i = entry.min + rand.next_int_bounded(1 + entry.max - entry.min);
        let mut j = center_x + rand.next_int_bounded(diameter_x);
        let mut k = center_z + rand.next_int_bounded(diameter_z);
        let (l, i1) = (j, k);
        for _ in 0..i {
            let mut flag = false;
            let mut k1 = 0;
            while !flag && k1 < 4 {
                if let Some(y) = terrain::top_solid_or_liquid(game, j, k) {
                    if can_creature_type_spawn_at_location(game, Placement::OnGround, j, y, k) {
                        let yaw = rand.next_float() * 360.0;
                        // DROPPED: a kind `MOBS` lacks is not created.
                        if let Some(def) = def_by_name(entry.name) {
                            spawn_initial(game, def, [(j as f32 + 0.5) as f64, y as f64, (k as f32 + 0.5) as f64], yaw);
                        }
                        flag = true;
                    }
                }
                j += rand.next_int_bounded(5) - rand.next_int_bounded(5);
                k += rand.next_int_bounded(5) - rand.next_int_bounded(5);
                // Vanilla bounds k with `centerZ + diameterX`.
                while j < center_x || j >= center_x + diameter_x || k < center_z || k >= center_z + diameter_x {
                    j = l + rand.next_int_bounded(5) - rand.next_int_bounded(5);
                    k = i1 + rand.next_int_bounded(5) - rand.next_int_bounded(5);
                }
                k1 += 1;
            }
        }
    }
}

// ---- despawning ----

/// `EntityLiving.canDespawn` for the kinds `MOBS` has: `EntityAnimal` says no (and the chicken's
/// jockey case is dropped), the monsters keep the default.
pub fn can_despawn(def: &MobDef) -> bool {
    lists::is_hostile(def)
}

/// `EntityLiving.despawnEntity` given the squared distance to the closest player (None when no
/// player is in the world). Returns true when the entity is set dead. `idle_time` is
/// `EntityLiving.idleTime`, already counted up for this tick.
pub fn despawn_entity(persistent: bool, idle_time: &mut i32, nearest_sq: Option<f64>, can_despawn: bool, rand: &mut JavaRandom) -> bool {
    if persistent {
        *idle_time = 0;
        return false;
    }
    let Some(d3) = nearest_sq else { return false };
    let mut dead = false;
    if can_despawn && d3 > DESPAWN_FAR_SQ {
        dead = true;
    }
    if *idle_time > DESPAWN_IDLE && rand.next_int_bounded(800) == 0 && d3 > DESPAWN_NEAR_SQ && can_despawn {
        dead = true;
    } else if d3 < DESPAWN_NEAR_SQ {
        *idle_time = 0;
    }
    dead
}

/// `World.getClosestPlayerToEntity(entity, -1)` squared: the nearest non-spectator.
pub fn nearest_player_sq(players: &[[f64; 3]], pos: [f64; 3]) -> Option<f64> {
    players
        .iter()
        .map(|p| {
            let (dx, dy, dz) = (p[0] - pos[0], p[1] - pos[1], p[2] - pos[2]);
            dx * dx + dy * dy + dz * dz
        })
        .min_by(|a, b| a.total_cmp(b))
}

#[cfg(test)]
#[path = "spawner_tests.rs"]
mod tests;
