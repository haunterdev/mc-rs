//! Epic Siege ESM-10: the hard-day spawner and the sleep denial, ported 1:1 from
//! `funwayguy/epicsiegemod/handlers/entities/PlayerHandler.java` (ESM 13.169).
//!
//! A day is `worldTime / 24000`; a hard day is `cycle != 0 && day != 0 && day % cycle == 0`
//! (`Hardcore Day Cycle`, 8 by default). On a hard day every survival player rolls, each tick, one
//! chance in ten: when it lands, one monster is placed at a random spot in the 48 x 48 x 48 box
//! around them, at least 8 blocks from every player, on a block vanilla's placement rules accept,
//! of a kind the biome's monster list holds, and only where its own `getCanSpawnHere` agrees (so a
//! lit spot or daylight still refuses). ESM's spawner answers to none of the per-chunk mob caps;
//! the 512 entity limit is its only ceiling.
//!
//! The step runs with the natural spawner, from [`mobs::spawn_step`](crate::game::mobs::spawn_step)
//! through the addon's `tick` hook, behind [`State::hard_days`] (on by default).
//!
//! DROPPED from `PlayerHandler`:
//! - `ForgeEventFactory.canEntitySpawn` (the hook has no filler here, so its `Event.Result.DEFAULT`
//!   is what `getCanSpawnHere` is asked to agree with) and the `try`/`catch` around the
//!   constructor (Rust has no reflection: a kind `MOBS` lacks is skipped after its draws).
//! - `onRespawn`, `onDimensionChange` and `onPlayerRespawn`: the `Resistance CoolDown` effect on
//!   join, dimension change and respawn, which is its own ESM system (section 7, "Respawn
//!   resistance"), not the spawner's.
//! - `onPlayerSleepInBed`, the bed hook: the server has no bed use to hang it on yet
//!   (`players.rs` knows a bed only as a block). Its day rule survives as [`sleep_denied`], which
//!   names what the hook owes when it lands.
//!
//! ESM's site rules are not vanilla's: `WorldEntitySpawner.canCreatureTypeSpawnAtLocation` is
//! checked, `EntityLiving.isNotColliding` is not, so two monsters can be placed on one spot and
//! one can be placed inside a player. The port keeps that.

use sim::collision::floor;
use worldgen::java_random::JavaRandom;

use crate::game::mobs::spawn_lists::{self as lists, CreatureType, Placement};
use crate::game::mobs::spawner;
use crate::game::mobs::{def_by_name, spawn};
use crate::game::food::Difficulty;
use crate::game::{terrain, Game};

/// ESM `Hardcore Day Cycle` (section 4): every 8th day is a hard day.
pub const HARD_DAY_CYCLE: i64 = 8;
/// ESM `Hardcore Day Cycle` 0: the hard days are off.
pub const NO_HARD_DAYS: i64 = 0;
/// `player.field_70170_p.field_72996_f.size() < 512`: the hard-day spawner's only ceiling.
pub const ENTITY_CAP: usize = 512;
/// The half-width of the box a spot is drawn from: `rand.nextInt(48) - 24` on each axis.
pub const SPAWN_BOX: i32 = 24;
/// The radius `World.isAnyPlayerWithinRangeAt(x, y, z, 8.0)` clears spawn sites by.
pub const PLAYER_CLEARANCE: f64 = 8.0;

/// What the hard-day spawner keeps between ticks.
pub struct State {
    /// The `hard_days` flag: ESM's `Hardcore Day Cycle` turned on, which is the default.
    pub hard_days: bool,
    /// Days between two hard days (`Hardcore Day Cycle`); 0 is ESM's second way to turn them off.
    pub cycle: i64,
}

impl Default for State {
    fn default() -> State {
        State { hard_days: true, cycle: HARD_DAY_CYCLE }
    }
}

/// `EsmConfigGlobal.hardDay != 0 && day != 0 && day % EsmConfigGlobal.hardDay == 0`.
pub fn hard_day(day: i64, cycle: i64) -> bool {
    cycle != NO_HARD_DAYS && day != 0 && day % cycle == 0
}

/// `(int)(player.world.getWorldTime() / 24000L)`: the day the world is on. `None` when no world is
/// attached, which is no day at all.
pub fn world_day(game: &Game) -> Option<i64> {
    Some(terrain::world(game)?.lock().unwrap().time() / 24000)
}

/// `PlayerHandler.onPlayerSleepInBed` with `Allow Sleep` false: on a hard day a bed refuses the
/// night. The original then sets the spawn point to the bed and says "Spawnpoint set", and leaves
/// the vanilla result alone when a monster stands within the 8 x 5 x 8 box around the bed.
///
/// DROPPED: the hook itself (`PlayerSleepInBedEvent`): the server has no bed use yet. When it
/// lands, the `OTHER_PROBLEM` result, `setSpawnChunk` and the chat line hang off this call, and
/// the monster box is read from the bed's own position.
///
/// The original denies the bed on every day once `Allow Sleep` is false; the day arithmetic here
/// is what ESM-10 asks for (a hard day is the day the siege is on).
pub fn sleep_denied(day: i64) -> bool {
    hard_day(day, HARD_DAY_CYCLE)
}

/// `World.loadedEntityList.size()`: every entity the world ticks (players, mobs, item stacks).
/// DROPPED: arrows and primed TNT, which the server has none of yet.
fn entity_count(game: &Game) -> usize {
    game.mobs.mobs.len() + game.items.entities.len() + game.xp_orbs.orbs.len() + game.sessions.len()
}

/// `World.isAnyPlayerWithinRangeAt(x, y, z, range)`: a non-spectator player closer than `range` to
/// the point, in blocks.
fn any_player_within(players: &[[f64; 3]], x: i32, y: i32, z: i32, range: f64) -> bool {
    players.iter().any(|p| {
        let (dx, dy, dz) = (p[0] - x as f64, p[1] - y as f64, p[2] - z as f64);
        dx * dx + dy * dy + dz * dz < range * range
    })
}

/// `World.getLightFor(EnumSkyBlock.SKY, pos)` as the chunk stores it, before
/// `skylightSubtracted`. `None` while the chunk is not final.
fn sky_light(game: &mut Game, x: i32, y: i32, z: i32) -> Option<i32> {
    if y < 0 {
        return Some(0);
    }
    terrain::light(game, (x, y.min(255), z)).map(|(sky, _)| sky as i32)
}

/// `EntityMob.getCanSpawnHere` for a monster about to stand at `(x, y, z)`: not peaceful,
/// `isValidLightLevel`, then `EntityCreature.getCanSpawnHere`'s block-path weight (which for an
/// `EntityMob` is `0.5 - getLightBrightness`). The natural spawner reads the same rule through its
/// own private copy in `spawner.rs`.
/// DROPPED: `EntityLiving.getCanSpawnHere`'s `Block.canEntitySpawn`, true for every 1.12.2 block.
fn can_spawn_here(game: &mut Game, ctx: &spawner::Ctx, x: i32, y: i32, z: i32, rand: &mut JavaRandom) -> bool {
    if ctx.difficulty == Difficulty::Peaceful {
        return false;
    }
    let Some(sky) = sky_light(game, x, y, z) else { return false };
    // isValidLightLevel reads the neighbours at skylightSubtracted 10 while it thunders.
    let sub = if ctx.thundering { 10 } else { ctx.sky_subtracted };
    let Some(light) = spawner::light_from_neighbors(game, x, y, z, sub) else { return false };
    if !spawner::valid_light_level(sky, light, rand) {
        return false;
    }
    let Some(plain) = spawner::light_from_neighbors(game, x, y, z, ctx.sky_subtracted) else { return false };
    0.5 - spawner::brightness(plain) >= 0.0
}

/// `PlayerHandler.onLivingUpdate`: on a hard day, each survival player is offered one extra
/// monster a tick, one time in ten.
pub fn step(game: &mut Game) {
    let st = &game.addons.epic_siege.hardday;
    if !st.hard_days {
        return;
    }
    let cycle = st.cycle;
    // The day is the same for every player, as it is in the original's per-player `hard`.
    let Some(day) = world_day(game) else { return };
    if !hard_day(day, cycle) {
        return;
    }
    let ctx = spawner::context(game);
    // The three generators the roll and the spot draw from, held out of `game` while the terrain
    // reads below borrow it. `players.rand` is `player.getRNG()`, the spawner's `world_rand` is
    // `WorldServer.rand` (the biome list's weighted pick) and `mobs.rand` stands in for the new
    // entity's own `Entity.rand` in `isValidLightLevel` (DROPPED: one generator per entity).
    let mut player_rand = std::mem::replace(&mut game.players.rand, JavaRandom::new(0));
    let mut world_rand = std::mem::replace(&mut game.mobs.spawner.world_rand, JavaRandom::new(0));
    let mut mob_rand = std::mem::replace(&mut game.mobs.rand, JavaRandom::new(0));
    for p in &ctx.players {
        offer_to_player(game, &ctx, *p, &mut player_rand, &mut world_rand, &mut mob_rand);
    }
    game.players.rand = player_rand;
    game.mobs.spawner.world_rand = world_rand;
    game.mobs.rand = mob_rand;
}

/// One player's roll in `onLivingUpdate`. The checks stand in the original's order, so the 10%
/// draw happens before the difficulty, game rule and entity-limit tests.
fn offer_to_player(game: &mut Game, ctx: &spawner::Ctx, p: [f64; 3], player_rand: &mut JavaRandom, world_rand: &mut JavaRandom, mob_rand: &mut JavaRandom) {
    if player_rand.next_int_bounded(10) != 0 {
        return;
    }
    if ctx.difficulty == Difficulty::Peaceful {
        return;
    }
    if !terrain::rule(game, "doMobSpawning", true) {
        return;
    }
    if entity_count(game) >= ENTITY_CAP {
        return;
    }
    let x = floor(p[0]) + player_rand.next_int_bounded(2 * SPAWN_BOX) - SPAWN_BOX;
    let y = floor(p[1]) + player_rand.next_int_bounded(2 * SPAWN_BOX) - SPAWN_BOX;
    let z = floor(p[2]) + player_rand.next_int_bounded(2 * SPAWN_BOX) - SPAWN_BOX;
    // `player.field_70170_p.func_184137_a((double)x, (double)y, (double)z, 8.0, false) == null`
    if any_player_within(&ctx.players, x, y, z, PLAYER_CLEARANCE) {
        return;
    }
    if !spawner::can_creature_type_spawn_at_location(game, Placement::OnGround, x, y, z) {
        return;
    }
    // `getSpawnListEntryForTypeAt(MONSTER, spawnPos)`: null for a biome whose monster list is
    // empty (a mushroom island), then the weighted pick from `World.rand`.
    let Some(biome) = terrain::biome(game, x, z) else { return };
    let list = lists::spawnable_list(biome, CreatureType::Monster);
    if list.is_empty() {
        return;
    }
    let entry = list[lists::pick(world_rand, &list)];
    // `setLocationAndAngles(x, y, z, rand.nextFloat() * 360.0F, 0.0F)`: the angle is drawn before
    // the spawn is decided, so a skipped kind still spends it.
    let yaw = player_rand.next_float() * 360.0;
    // DROPPED: a kind `MOBS` lacks (slime, enderman, witch, villager zombie, husk, stray) is
    // constructed and asked for its `getCanSpawnHere` in the original; here it is skipped.
    let Some(def) = def_by_name(entry.name) else { return };
    if can_spawn_here(game, ctx, x, y, z, mob_rand) {
        // `world.spawnEntity(entityliving)`: plain, with no `onInitialSpawn` (ESM never calls it).
        spawn(game, def, [x as f64, y as f64, z as f64], yaw);
    }
}

#[cfg(test)]
#[path = "hardday_tests.rs"]
mod tests;
