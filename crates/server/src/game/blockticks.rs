//! Random block ticks and what the blocks do with them: `WorldServer.updateBlocks`' loop over the
//! loaded sections, and the `updateTick` of the growing blocks (`BlockCrops`, `BlockStem`,
//! `BlockFarmland`, `BlockGrass`, `BlockReed`, `BlockCactus`, `BlockSapling`, `BlockNetherWart`,
//! `BlockLeaves`) plus the `IGrowable` half bone meal uses ([`bonemeal`]). Every change goes
//! through the terrain's `set_flags`, so the viewers get it as `S_BLOCKS` and what a change
//! breaks drops through `mining::neighbour_drops`.
//!
//! DROPPED: `ExtendedBlockStorage.needsRandomTick` (a section is ticked when it has storage, the
//! draws land on blocks that do not tick and do nothing), the rain/snow/ice and lightning part of
//! `updateBlocks` (`nextInt(16)` precipitation, `nextInt(100000)` strikes), the random ticks of
//! every block not named above (fire, vines, liquids, mushrooms, cocoa, lava), a tick whose chunk
//! light is not computed yet (the 3x3 around it is not final), `useNeighborBrightness` of
//! `World.getLight` (slabs, farmland), the continuation of `BlockCrops.updateTick` after the block
//! removed itself (it would plant a crop into the air), the flower choice of
//! `Biome.pickRandomFlower` past the base dandelion/poppy, the entity lift of
//! `BlockFarmland.turnToDirt`, the trampling by mobs (they have no fall distance), `notify`
//! flag 3 on a tree's blocks (flag 2 is written, the viewers still get all of them), cocoa and
//! mushrooms' bone meal.

use serde_json::Value;
use sim::placement::is_replaceable;
use worldgen::blocks::{self, id};
use worldgen::java_random::JavaRandom;
use worldgen::populate::state as pstate;
use worldgen::populate::trees::{self, Tree};
use worldgen::populate::Pos as WPos;

use super::terrain::{self, Blocks};
use super::{mining, mobs, Game};

const AIR: u16 = blocks::AIR;
const NETHER_WART: u16 = 115;
const SOUL_SAND: u16 = 88;

#[derive(Default)]
pub struct State {
    /// `WorldServer.updateLCG`.
    pub lcg: Option<i32>,
    /// `World.rand` for the growth rolls.
    pub rand: Option<JavaRandom>,
    /// `fastLeafDecay`: ticks counted, and the leaves due a decay check at a tick.
    pub now: u64,
    pub pending: Vec<(u64, (i32, i32, i32))>,
}

pub fn on_join(_game: &mut Game, _id: u32) {}

pub fn on_leave(_game: &mut Game, _id: u32) {}

pub fn on_frame(_game: &mut Game, _id: u32, _tag: u8, _body: &[u8]) -> bool {
    false
}

pub fn save(_game: &Game) -> Value {
    Value::Null
}

pub fn load(_game: &mut Game, _v: Value) {}

/// `f` with the growth random stream, put back after.
pub fn with_rand<R>(game: &mut Game, f: impl FnOnce(&mut Game, &mut JavaRandom) -> R) -> R {
    let mut rand = game.blockticks.rand.take().unwrap_or_else(|| JavaRandom::new(crate::config::random_u64() as i64));
    let out = f(game, &mut rand);
    game.blockticks.rand = Some(rand);
    out
}

/// `GameRules.getInt("randomTickSpeed")`, 3 by default.
fn random_tick_speed(game: &Game) -> i32 {
    let Some(w) = terrain::world(game) else { return 3 };
    let w = w.lock().unwrap();
    w.rule("randomTickSpeed").and_then(|v| v.parse().ok()).unwrap_or(3)
}

/// Blocks with `getTickRandomly()` that this module ticks.
fn ticks_randomly(b: u16) -> bool {
    matches!(
        b,
        id::GRASS
            | id::FARMLAND
            | id::WHEAT
            | id::CARROTS
            | id::POTATOES
            | id::BEETROOTS
            | id::PUMPKIN_STEM
            | id::MELON_STEM
            | id::REEDS
            | id::CACTUS
            | id::SAPLING
            | id::LEAVES
            | id::LEAVES2
            | NETHER_WART
    )
}

/// `WorldServer.updateBlocks`: per chunk of a player's surroundings, per section with storage,
/// `randomTickSpeed` draws of `updateLCG` pick a cell, and a block that ticks randomly gets it.
pub fn tick(game: &mut Game) {
    fast_leaf_decay(game);
    let speed = random_tick_speed(game);
    if speed <= 0 {
        return;
    }
    let sim = game.cfg.sim_distance.max(0);
    let centers = terrain::centers(game);
    if centers.is_empty() {
        return;
    }
    let mut lcg = game.blockticks.lcg.unwrap_or_else(|| JavaRandom::new(crate::config::random_u64() as i64).next_int());
    let chunks = terrain::plan(&centers, sim);
    with_rand(game, |game, rand| {
        for c in chunks {
            if !terrain::is_final(game, c) {
                continue;
            }
            for sy in 0..16usize {
                if !terrain::has_section(game, c.0, c.1, sy) {
                    continue;
                }
                for _ in 0..speed {
                    lcg = lcg.wrapping_mul(3).wrapping_add(1013904223);
                    let j1 = lcg >> 2;
                    let p = (c.0 * 16 + (j1 & 15), sy as i32 * 16 + ((j1 >> 16) & 15), c.1 * 16 + ((j1 >> 8) & 15));
                    let s = terrain::state(game, p);
                    if ticks_randomly(s >> 4) {
                        random_tick(game, p, rand);
                    }
                }
            }
        }
    });
    game.blockticks.lcg = Some(lcg);
}

/// The `fastLeafDecay` setting: a leaf that started checking decay runs `BlockLeaves.updateTick`
/// when its scheduled tick (2 to 10 ticks after the log broke) is due, as a scheduled tick would,
/// instead of waiting for a random tick. A leaf that decays marks its neighbours as vanilla does.
fn fast_leaf_decay(game: &mut Game) {
    game.blockticks.now += 1;
    if game.blockticks.pending.is_empty() {
        return;
    }
    let now = game.blockticks.now;
    let (due, later): (Vec<_>, Vec<_>) = std::mem::take(&mut game.blockticks.pending).into_iter().partition(|(t, _)| *t <= now);
    game.blockticks.pending = later;
    for (_, p) in due {
        let s = terrain::state(game, p);
        if is_leaves(s >> 4) {
            leaves_tick(game, p, s);
        }
    }
}

/// `Block.randomTick` -> `updateTick` of the block at `p`.
pub fn random_tick(game: &mut Game, p: (i32, i32, i32), rand: &mut JavaRandom) {
    let s = terrain::state(game, p);
    match s >> 4 {
        id::GRASS => grass_tick(game, p, rand),
        id::FARMLAND => farmland_tick(game, p, s),
        id::WHEAT | id::CARROTS | id::POTATOES => crop_tick(game, p, s, rand),
        id::BEETROOTS => {
            // BlockBeetroot.updateTick
            if rand.next_int_bounded(3) == 0 {
                check_and_drop_crop(game, p);
            } else {
                crop_tick(game, p, s, rand);
            }
        }
        id::PUMPKIN_STEM | id::MELON_STEM => stem_tick(game, p, s, rand),
        id::REEDS => reed_tick(game, p, s),
        id::CACTUS => cactus_tick(game, p, s),
        id::SAPLING => sapling_tick(game, p, s, rand),
        id::LEAVES | id::LEAVES2 => leaves_tick(game, p, s),
        NETHER_WART => wart_tick(game, p, s, rand),
        _ => {}
    }
}

// ---- reads and writes ----

fn up(p: (i32, i32, i32)) -> (i32, i32, i32) {
    (p.0, p.1 + 1, p.2)
}

fn down(p: (i32, i32, i32)) -> (i32, i32, i32) {
    (p.0, p.1 - 1, p.2)
}

fn block_at(game: &Game, p: (i32, i32, i32)) -> u16 {
    terrain::state(game, p) >> 4
}

/// `World.getLightFromNeighbors(p)`: the brighter of the sky light less the daylight dimming and
/// the block light. None until the chunk's light is known.
fn light_from_neighbors(game: &mut Game, p: (i32, i32, i32)) -> Option<i32> {
    let sub = mobs::spawner::sky_subtracted(game);
    terrain::light(game, p).map(|(s, b)| (s as i32 - sub).max(b as i32))
}

/// `World.getLight(p)`.
fn light_at(game: &mut Game, p: (i32, i32, i32)) -> Option<i32> {
    terrain::light(game, p).map(|(s, b)| s.max(b) as i32)
}

/// `World.canSeeSky(p)`.
fn can_see_sky(game: &Game, p: (i32, i32, i32)) -> bool {
    terrain::column(game, p.0, p.2).is_some_and(|(h, _)| p.1 >= h)
}

/// `World.isBlockLoaded(p)`.
fn loaded(game: &Game, p: (i32, i32, i32)) -> bool {
    terrain::column(game, p.0, p.2).is_some()
}

/// `World.setBlockState(p, s, flags)`; what the change broke drops.
fn set(game: &mut Game, p: (i32, i32, i32), s: u16, flags: i32) {
    let (changes, olds) = terrain::set_flags(game, p, s, flags);
    if changes.len() > 1 {
        mining::neighbour_drops(game, &changes[1..], &olds[1..]);
    }
}

/// `dropBlockAsItem` then `setBlockState(p, AIR, 3)`.
fn remove(game: &mut Game, p: (i32, i32, i32)) {
    let (changes, olds) = terrain::set_flags(game, p, AIR, 3);
    mining::neighbour_drops(game, &changes, &olds);
}

/// `BlockBush.canSustainBush`: grass, dirt or farmland below.
fn bush_soil(game: &Game, p: (i32, i32, i32)) -> bool {
    matches!(block_at(game, down(p)), id::GRASS | id::DIRT | id::FARMLAND)
}

// ---- crops ----

fn crop_max_age(b: u16) -> i32 {
    if b == id::BEETROOTS {
        3
    } else {
        7
    }
}

/// `BlockCrops.canBlockStay`.
fn crop_can_stay(game: &mut Game, p: (i32, i32, i32)) -> bool {
    let lit = light_at(game, p).map_or(true, |l| l >= 8) || can_see_sky(game, p);
    lit && block_at(game, down(p)) == id::FARMLAND
}

/// `BlockBush.checkAndDropBlock` for a crop. True when it stays.
fn check_and_drop_crop(game: &mut Game, p: (i32, i32, i32)) -> bool {
    if crop_can_stay(game, p) {
        return true;
    }
    remove(game, p);
    false
}

/// `BlockCrops.getGrowthChance` (also the stems').
fn growth_chance(game: &Game, b: u16, p: (i32, i32, i32)) -> f32 {
    let mut f = 1.0f32;
    let below = down(p);
    for i in -1..=1 {
        for j in -1..=1 {
            let s = terrain::state(game, (below.0 + i, below.1, below.2 + j));
            let mut f1 = 0.0f32;
            if s >> 4 == id::FARMLAND {
                f1 = 1.0;
                if s & 15 > 0 {
                    f1 = 3.0;
                }
            }
            if i != 0 || j != 0 {
                f1 /= 4.0;
            }
            f += f1;
        }
    }
    let same = |dx: i32, dz: i32| block_at(game, (p.0 + dx, p.1, p.2 + dz)) == b;
    let flag = same(-1, 0) || same(1, 0);
    let flag1 = same(0, -1) || same(0, 1);
    if flag && flag1 || same(-1, -1) || same(1, -1) || same(1, 1) || same(-1, 1) {
        f /= 2.0;
    }
    f
}

/// `BlockCrops.updateTick`.
fn crop_tick(game: &mut Game, p: (i32, i32, i32), s: u16, rand: &mut JavaRandom) {
    if !check_and_drop_crop(game, p) {
        return;
    }
    let b = s >> 4;
    let Some(l) = light_from_neighbors(game, up(p)) else { return };
    if l >= 9 {
        let age = (s & 15) as i32;
        if age < crop_max_age(b) {
            let f = growth_chance(game, b, p);
            if rand.next_int_bounded((25.0f32 / f) as i32 + 1) == 0 {
                set(game, p, (b << 4) | (age + 1) as u16, 2);
            }
        }
    }
}

/// `BlockStem.updateTick`.
fn stem_tick(game: &mut Game, p: (i32, i32, i32), s: u16, rand: &mut JavaRandom) {
    let b = s >> 4;
    // BlockBush.checkAndDropBlock: a stem needs farmland below, no light.
    if block_at(game, down(p)) != id::FARMLAND {
        remove(game, p);
        return;
    }
    let Some(l) = light_from_neighbors(game, up(p)) else { return };
    if l < 9 {
        return;
    }
    let f = growth_chance(game, b, p);
    if rand.next_int_bounded((25.0f32 / f) as i32 + 1) != 0 {
        return;
    }
    let age = (s & 15) as i32;
    if age < 7 {
        set(game, p, (b << 4) | (age + 1) as u16, 2);
        return;
    }
    let fruit = if b == id::PUMPKIN_STEM { id::PUMPKIN } else { id::MELON_BLOCK };
    for f in worldgen::populate::Facing::HORIZONTAL {
        let (ox, _, oz) = f.offset();
        if block_at(game, (p.0 + ox, p.1, p.2 + oz)) == fruit {
            return;
        }
    }
    let f = worldgen::populate::Facing::random_horizontal(rand);
    let (ox, _, oz) = f.offset();
    let q = (p.0 + ox, p.1, p.2 + oz);
    let soil = block_at(game, down(q));
    if is_replaceable(&Blocks(game), q) && matches!(soil, id::FARMLAND | id::DIRT | id::GRASS) {
        // The pumpkin's default facing is north (meta 2); the melon has no state.
        let meta = if fruit == id::PUMPKIN { 2 } else { 0 };
        set(game, q, blocks::state(fruit, meta), 3);
    }
}

/// `BlockNetherWart.updateTick`.
fn wart_tick(game: &mut Game, p: (i32, i32, i32), s: u16, rand: &mut JavaRandom) {
    let age = (s & 15) as i32;
    if age < 3 && rand.next_int_bounded(10) == 0 {
        set(game, p, (NETHER_WART << 4) | (age + 1) as u16, 2);
    }
    if block_at(game, down(p)) != SOUL_SAND {
        remove(game, p);
    }
}

// ---- farmland ----

/// `BlockFarmland.hasWater`: water in x -4..4, y 0..1, z -4..4.
fn has_water(game: &Game, p: (i32, i32, i32)) -> bool {
    for x in p.0 - 4..=p.0 + 4 {
        for y in p.1..=p.1 + 1 {
            for z in p.2 - 4..=p.2 + 4 {
                if matches!(block_at(game, (x, y, z)), id::FLOWING_WATER | id::WATER) {
                    return true;
                }
            }
        }
    }
    false
}

/// `BlockFarmland.hasCrops`: a crop or stem above.
fn has_crops(game: &Game, p: (i32, i32, i32)) -> bool {
    matches!(block_at(game, up(p)), id::WHEAT | id::CARROTS | id::POTATOES | id::BEETROOTS | id::PUMPKIN_STEM | id::MELON_STEM)
}

/// `World.isRainingAt(p)`.
fn is_raining_at(game: &mut Game, p: (i32, i32, i32)) -> bool {
    if !terrain::world(game).is_some_and(|w| w.lock().unwrap().weather.raining) {
        return false;
    }
    terrain::peek(game, |w| {
        let q = WPos::new(p.0, p.1, p.2);
        if !w.can_see_sky(q) || w.precipitation_height(q).y > p.1 {
            return false;
        }
        let biome = w.biome(q);
        // Biome.canRain: a biome that never rains has rainfall 0.
        !biome.snowy && !w.can_snow_at(q) && biome.rainfall > 0.0
    })
    .unwrap_or(false)
}

/// `BlockFarmland.turnToDirt`.
pub fn turn_to_dirt(game: &mut Game, p: (i32, i32, i32)) {
    set(game, p, blocks::DIRT, 3);
}

/// `BlockFarmland.updateTick`.
fn farmland_tick(game: &mut Game, p: (i32, i32, i32), s: u16) {
    let moisture = s & 7;
    if !has_water(game, p) && !is_raining_at(game, up(p)) {
        if moisture > 0 {
            set(game, p, (id::FARMLAND << 4) | (moisture - 1), 2);
        } else if !has_crops(game, p) {
            turn_to_dirt(game, p);
        }
    } else if moisture < 7 {
        set(game, p, (id::FARMLAND << 4) | 7, 2);
    }
}

/// `BlockFarmland.onFallenUpon` for a player: a hard enough landing turns it to dirt.
pub fn farmland_fallen_upon(game: &mut Game, p: (i32, i32, i32), distance: f32) {
    if block_at(game, p) != id::FARMLAND {
        return;
    }
    let hit = with_rand(game, |_, rand| rand.next_float() < distance - 0.5);
    if hit {
        turn_to_dirt(game, p);
    }
}

// ---- grass ----

/// `BlockGrass.updateTick`.
fn grass_tick(game: &mut Game, p: (i32, i32, i32), rand: &mut JavaRandom) {
    let Some(l) = light_from_neighbors(game, up(p)) else { return };
    if l < 4 && pstate::opacity(terrain::state(game, up(p))) > 2 {
        set(game, p, blocks::DIRT, 3);
    } else if l >= 9 {
        for _ in 0..4 {
            let q = (p.0 + rand.next_int_bounded(3) - 1, p.1 + rand.next_int_bounded(5) - 3, p.2 + rand.next_int_bounded(3) - 1);
            if q.1 >= 0 && q.1 < 256 && !loaded(game, q) {
                return;
            }
            let above = terrain::state(game, up(q));
            let here = terrain::state(game, q);
            if here == blocks::DIRT && light_from_neighbors(game, up(q)).is_some_and(|l| l >= 4) && pstate::opacity(above) <= 2 {
                set(game, q, blocks::GRASS, 3);
            }
        }
    }
}

/// `BlockGrass.grow`: bone meal scatters tall grass and flowers around the block.
fn grass_grow(game: &mut Game, p: (i32, i32, i32), rand: &mut JavaRandom) {
    let top = up(p);
    for i in 0..128 {
        let mut q = top;
        let mut j = 0;
        loop {
            if j >= i / 16 {
                if terrain::state(game, q) >> 4 == 0 {
                    if rand.next_int_bounded(8) == 0 {
                        // Biome.pickRandomFlower base.
                        let flower = if rand.next_int_bounded(3) > 0 { id::YELLOW_FLOWER } else { id::RED_FLOWER };
                        if bush_soil(game, q) {
                            set(game, q, flower << 4, 3);
                        }
                    } else if bush_soil(game, q) {
                        set(game, q, blocks::state(id::TALLGRASS, 1), 3);
                    }
                }
                break;
            }
            let dx = rand.next_int_bounded(3) - 1;
            let dy = (rand.next_int_bounded(3) - 1) * rand.next_int_bounded(3) / 2;
            let dz = rand.next_int_bounded(3) - 1;
            q = (q.0 + dx, q.1 + dy, q.2 + dz);
            if block_at(game, down(q)) != id::GRASS || pstate::is_normal_cube(terrain::state(game, q)) {
                break;
            }
            j += 1;
        }
    }
}

// ---- reeds and cactus ----

/// `BlockReed.canPlaceBlockAt` for a reed in place: reeds below, or sand/dirt/grass with water
/// beside its foot.
fn reed_can_stay(game: &Game, p: (i32, i32, i32)) -> bool {
    let d = down(p);
    match block_at(game, d) {
        id::REEDS => true,
        id::GRASS | id::DIRT | id::SAND => [(-1, 0), (1, 0), (0, -1), (0, 1)]
            .iter()
            .any(|&(dx, dz)| pstate::mat(terrain::state(game, (d.0 + dx, d.1, d.2 + dz))) == worldgen::populate::props::material::WATER),
        _ => false,
    }
}

/// `BlockReed.updateTick`.
fn reed_tick(game: &mut Game, p: (i32, i32, i32), s: u16) {
    if block_at(game, down(p)) != id::REEDS && !reed_can_stay(game, p) {
        remove(game, p);
        return;
    }
    if block_at(game, up(p)) != 0 {
        return;
    }
    let mut i = 1;
    while block_at(game, (p.0, p.1 - i, p.2)) == id::REEDS {
        i += 1;
    }
    if i < 3 {
        let age = s & 15;
        if age == 15 {
            set(game, up(p), id::REEDS << 4, 3);
            set(game, p, id::REEDS << 4, 4);
        } else {
            set(game, p, (id::REEDS << 4) | (age + 1), 4);
        }
    }
}

/// `BlockCactus.canBlockStay`.
fn cactus_can_stay(game: &Game, p: (i32, i32, i32)) -> bool {
    for (dx, dz) in [(0, -1), (1, 0), (0, 1), (-1, 0)] {
        let s = terrain::state(game, (p.0 + dx, p.1, p.2 + dz));
        if pstate::mat_solid(s) || pstate::mat(s) == worldgen::populate::props::material::LAVA {
            return false;
        }
    }
    matches!(block_at(game, down(p)), id::CACTUS | id::SAND) && !pstate::mat_liquid(terrain::state(game, up(p)))
}

/// `BlockCactus.updateTick`.
fn cactus_tick(game: &mut Game, p: (i32, i32, i32), s: u16) {
    let top = up(p);
    if block_at(game, top) != 0 {
        return;
    }
    let mut i = 1;
    while block_at(game, (p.0, p.1 - i, p.2)) == id::CACTUS {
        i += 1;
    }
    if i < 3 {
        let age = s & 15;
        if age == 15 {
            set(game, top, id::CACTUS << 4, 3);
            set(game, p, id::CACTUS << 4, 4);
            // iblockstate.neighborChanged(world, blockpos, this, pos): the new top checks itself.
            if !cactus_can_stay(game, top) {
                remove(game, top);
            }
        } else {
            set(game, p, (id::CACTUS << 4) | (age + 1), 4);
        }
    }
}

// ---- saplings ----

/// `BlockSapling.isTwoByTwoOfType`.
fn two_by_two(game: &Game, p: (i32, i32, i32), i: i32, j: i32, ty: u16) -> bool {
    [(0, 0), (1, 0), (0, 1), (1, 1)].iter().all(|&(a, b)| {
        let s = terrain::state(game, (p.0 + i + a, p.1, p.2 + j + b));
        s >> 4 == id::SAPLING && s & 7 == ty
    })
}

/// The `i`, `j` loops of `BlockSapling.generateTree`: i, j from 0 down to -1.
fn find_two_by_two(game: &Game, p: (i32, i32, i32), ty: u16) -> Option<(i32, i32)> {
    for i in [0, -1] {
        for j in [0, -1] {
            if two_by_two(game, p, i, j, ty) {
                return Some((i, j));
            }
        }
    }
    None
}

/// `BlockSapling.updateTick`.
fn sapling_tick(game: &mut Game, p: (i32, i32, i32), s: u16, rand: &mut JavaRandom) {
    if !bush_soil(game, p) {
        remove(game, p);
        return;
    }
    let Some(l) = light_from_neighbors(game, up(p)) else { return };
    if l >= 9 && rand.next_int_bounded(7) == 0 {
        sapling_grow(game, p, s, rand);
    }
}

/// `BlockSapling.grow`: the stage, then the tree.
fn sapling_grow(game: &mut Game, p: (i32, i32, i32), s: u16, rand: &mut JavaRandom) {
    if s & 8 == 0 {
        set(game, p, s | 8, 4);
    } else {
        generate_tree(game, p, s, rand);
    }
}

/// `BlockSapling.generateTree`. True when a tree stands where the sapling was.
pub fn generate_tree(game: &mut Game, p: (i32, i32, i32), s: u16, rand: &mut JavaRandom) -> bool {
    let ty = s & 7;
    let mut tree = if rand.next_int_bounded(10) == 0 {
        Tree::Big
    } else {
        Tree::Trees { min_height: 4, wood: trees::OAK_LOG, leaves: trees::OAK_LEAF, vines: false }
    };
    let (mut i, mut j, mut flag) = (0, 0, false);
    match ty {
        1 => {
            if let Some((a, b)) = find_two_by_two(game, p, ty) {
                (i, j, flag) = (a, b, true);
                tree = Tree::MegaPine { use_base_height: rand.next_boolean() };
            } else {
                tree = Tree::Taiga2;
            }
        }
        2 => tree = Tree::Birch { extra: false },
        3 => {
            if let Some((a, b)) = find_two_by_two(game, p, ty) {
                (i, j, flag) = (a, b, true);
                tree = Tree::MegaJungle { base: 10, extra: 20, wood: trees::JUNGLE_LOG, leaves: trees::JUNGLE_LEAF };
            } else {
                tree = Tree::Trees { min_height: 4 + rand.next_int_bounded(7), wood: trees::JUNGLE_LOG, leaves: trees::JUNGLE_LEAF, vines: false };
            }
        }
        4 => tree = Tree::Savanna,
        5 => {
            let Some((a, b)) = find_two_by_two(game, p, ty) else { return false };
            (i, j, flag) = (a, b, true);
            tree = Tree::Canopy;
        }
        _ => {}
    }
    let cells: Vec<((i32, i32, i32), u16)> = if flag {
        [(0, 0), (1, 0), (0, 1), (1, 1)].iter().map(|&(a, b)| ((p.0 + i + a, p.1, p.2 + j + b), terrain::state(game, (p.0 + i + a, p.1, p.2 + j + b)))).collect()
    } else {
        vec![(p, terrain::state(game, p))]
    };
    let at = (p.0 + i, p.1, p.2 + j);
    let grown = terrain::edit_world(game, at, |w| {
        for &(c, _) in &cells {
            w.set(WPos::new(c.0, c.1, c.2), AIR, 4);
        }
        tree.set_decoration_defaults(w);
        let ok = tree.generate(w, rand, WPos::new(at.0, at.1, at.2));
        if !ok {
            for &(c, old) in &cells {
                w.set(WPos::new(c.0, c.1, c.2), old, 4);
            }
        }
        ok
    });
    grown.is_some_and(|(ok, _)| ok)
}

// ---- leaves ----

fn is_log(b: u16) -> bool {
    b == id::LOG || b == id::LOG2
}

fn is_leaves(b: u16) -> bool {
    b == id::LEAVES || b == id::LEAVES2
}

/// `World.isAreaLoaded` for the box `r` around `p` (by its corner columns).
fn area_loaded(game: &Game, p: (i32, i32, i32), r: i32) -> bool {
    [(-r, -r), (r, -r), (-r, r), (r, r)].iter().all(|&(dx, dz)| loaded(game, (p.0 + dx, p.1, p.2 + dz)))
}

/// `BlockLeaves.updateTick`: leaves marked `check_decay` and decayable look for a log within four
/// leaves and drop out of the tree when none is there.
fn leaves_tick(game: &mut Game, p: (i32, i32, i32), s: u16) {
    if s & 8 == 0 || s & 4 != 0 {
        return;
    }
    if !area_loaded(game, p, 5) {
        return;
    }
    // surroundings[(x + 16) * 1024 + (y + 16) * 32 + z + 16]: 0 a log, -2 a leaf, -1 else.
    let mut sur = vec![-1i32; 32768];
    let idx = |x: i32, y: i32, z: i32| ((x + 16) * 1024 + (y + 16) * 32 + z + 16) as usize;
    for x in -4..=4 {
        for y in -4..=4 {
            for z in -4..=4 {
                let b = block_at(game, (p.0 + x, p.1 + y, p.2 + z));
                sur[idx(x, y, z)] = if is_log(b) {
                    0
                } else if is_leaves(b) {
                    -2
                } else {
                    -1
                };
            }
        }
    }
    for k in 1..=4 {
        for x in -4..=4 {
            for y in -4..=4 {
                for z in -4..=4 {
                    if sur[idx(x, y, z)] == k - 1 {
                        for (dx, dy, dz) in [(-1, 0, 0), (1, 0, 0), (0, -1, 0), (0, 1, 0), (0, 0, -1), (0, 0, 1)] {
                            let n = idx(x + dx, y + dy, z + dz);
                            if sur[n] == -2 {
                                sur[n] = k;
                            }
                        }
                    }
                }
            }
        }
    }
    if sur[16912] >= 0 {
        set(game, p, s & !8, 4);
    } else {
        remove(game, p);
    }
}

/// `BlockLog.breakBlock`: every leaf block within four of the log starts checking for decay
/// (`beginLeavesDecay`).
pub fn log_broken(game: &mut Game, p: (i32, i32, i32)) {
    if !area_loaded(game, p, 5) {
        return;
    }
    for x in -4..=4 {
        for y in -4..=4 {
            for z in -4..=4 {
                let q = (p.0 + x, p.1 + y, p.2 + z);
                let s = terrain::state(game, q);
                if is_leaves(s >> 4) && s & 8 == 0 {
                    set(game, q, s | 8, 4);
                    if game.cfg.fast_leaf_decay {
                        // 2 to 10 ticks out.
                        let delay = with_rand(game, |_, r| r.next_int_bounded(9)) as u64;
                        let due = game.blockticks.now + 2 + delay;
                        game.blockticks.pending.push((due, q));
                    }
                }
            }
        }
    }
}

// ---- bone meal ----

fn tall_grass_can_grow(s: u16) -> bool {
    s >> 4 == id::TALLGRASS && s & 15 != 0
}

/// `IGrowable.canGrow` of the block at `p`.
pub fn can_grow(game: &Game, p: (i32, i32, i32)) -> bool {
    let s = terrain::state(game, p);
    match s >> 4 {
        id::WHEAT | id::CARROTS | id::POTATOES | id::BEETROOTS => ((s & 15) as i32) < crop_max_age(s >> 4),
        id::PUMPKIN_STEM | id::MELON_STEM => s & 15 != 7,
        id::SAPLING | id::GRASS => true,
        id::TALLGRASS => tall_grass_can_grow(s),
        _ => false,
    }
}

/// `ItemDye.applyBonemeal`: true when the block could grow (the dye is used up), whether or not
/// this use made it.
pub fn bonemeal(game: &mut Game, p: (i32, i32, i32)) -> bool {
    if !can_grow(game, p) {
        return false;
    }
    with_rand(game, |game, rand| {
        let s = terrain::state(game, p);
        let b = s >> 4;
        match b {
            id::WHEAT | id::CARROTS | id::POTATOES | id::BEETROOTS => {
                // BlockCrops.grow: age + getInt(rand, 2, 5), the beetroot a third of it.
                let mut inc = rand.next_int_bounded(4) + 2;
                if b == id::BEETROOTS {
                    inc /= 3;
                }
                let age = ((s & 15) as i32 + inc).min(crop_max_age(b));
                set(game, p, (b << 4) | age as u16, 2);
            }
            id::PUMPKIN_STEM | id::MELON_STEM => {
                let age = ((s & 15) as i32 + rand.next_int_bounded(4) + 2).min(7);
                set(game, p, (b << 4) | age as u16, 2);
            }
            id::SAPLING => {
                // canUseBonemeal: nextFloat < 0.45
                if (rand.next_float() as f64) < 0.45 {
                    sapling_grow(game, p, s, rand);
                }
            }
            id::GRASS => grass_grow(game, p, rand),
            id::TALLGRASS => {
                // BlockTallGrass.grow: a double plant in place of the tall grass.
                let lower = if s & 15 == 2 { 3 } else { 2 };
                if bush_soil(game, p) && block_at(game, up(p)) == 0 {
                    set(game, p, blocks::state(id::DOUBLE_PLANT, lower), 2);
                    set(game, up(p), blocks::state(id::DOUBLE_PLANT, 10), 2);
                }
            }
            _ => {}
        }
    });
    true
}
