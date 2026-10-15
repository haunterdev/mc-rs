//! Per-player survival state: health, hunger, exhaustion, falling, air, lava and fire, the void.
//! `EntityPlayerMP.onUpdate` as [`tick`], `NetHandlerPlayServer.processPlayer`'s stat, jump and
//! fall branch as [`on_state`] (the viewer moves itself, so the server never simulates the
//! player's motion; these read the deltas between the states it sends). Damage and death are
//! damage.rs. The health frame goes out when it differs from the last one sent
//! (`EntityPlayerMP.onUpdateEntity`).
//!
//! DROPPED across the file: statistics, advancements and score criteria, levitation, the world
//! border, riding, sleeping, elytra and shoulder entities: none of them exist in this server yet.
//! Potion effects are effects.rs.

use std::collections::HashMap;

use items::ItemStack;
use proto::{PlayerState, ON_GROUND, SNEAKING, SPRINTING};
use serde_json::Value;
use sim::collision::{floor, handle_material_acceleration, is_material_in_bb, is_normal_cube, Aabb};
use sim::inventory::InventoryPlayer;
use sim::region::{liquid_height_percent, props, Mat};
use worldgen::java_random::JavaRandom;
use worldgen::math_helper::{cos, sin};

use super::damage::{self, Combat, DamageSource};
use super::food::{Difficulty, FoodStats};
use super::playerdata;
use super::terrain::{self, Blocks};
use super::{items as item_entities, net, Game};

/// `EntityLivingBase`'s `GENERIC_MAX_HEALTH`.
pub const MAX_HEALTH: f32 = 20.0;
/// `EntityPlayerMP.respawnInvulnerabilityTicks`.
pub const RESPAWN_INVULNERABLE: i32 = 60;
/// `EntityLivingBase.setAir(300)`.
pub const AIR_MAX: i32 = 300;

/// `Block` ids the survival rules look at.
const BED: u16 = 26;
const COBWEB: u16 = 30;
const FARMLAND: u16 = 60;
const LADDER: u16 = 65;
const FENCE: u16 = 85;
const FENCE_GATE: u16 = 107;
const NETHER_BRICK_FENCE: u16 = 113;
const COBBLESTONE_WALL: u16 = 139;
const SLIME: u16 = 165;
const HAY: u16 = 170;
const VINE: u16 = 106;

/// One player's survival state: the fields of `EntityPlayerMP`, `EntityPlayer` and
/// `EntityLivingBase` that health, hunger and damage read.
pub struct Survival {
    pub health: f32,
    /// `EntityLivingBase.getMaxHealth`: 20 plus the health boost modifier.
    pub max_health: f32,
    /// `EntityLivingBase.getAbsorptionAmount`
    pub absorption: f32,
    /// `EntityLivingBase.activePotionsMap`
    pub effects: items::potion::Effects,
    pub food: FoodStats,
    pub air: i32,
    pub fall_distance: f32,
    /// `Entity.fire`, in ticks.
    pub fire: i32,
    /// `Entity.inWater` as the last tick found it.
    pub in_water: bool,
    pub hurt_resistant: i32,
    pub last_damage: f32,
    pub respawn_invulnerable: i32,
    pub ticks_existed: i32,
    /// Health is 0 and the player has not respawned.
    pub dead: bool,
    pub xp: proto::Xp,
    /// `EntityPlayer.xpSeed`: seeds the enchanting table's offers; `onEnchant` draws a new one.
    pub xp_seed: i32,
    /// `EntityPlayer.xpCooldown`: ticks before the next orb pickup.
    pub xp_cooldown: i32,
    /// `EntityPlayer.lastXPSound`: the `ticksExisted` of the last level-up sound.
    pub last_xp_sound: i32,
    /// `EntityLivingBase.deathTime`: ticks since the player died; the experience orbs drop at 20.
    pub death_time: i32,
    pub inv: InventoryPlayer,
    pub combat: Combat,
    /// `EntityPlayer.ticksSinceLastSwing`: the attack cooldown counter.
    pub ticks_since_last_swing: i32,
    /// `EntityPlayer.itemStackMainHand`: the stack the cooldown last saw in hand.
    pub main_hand: ItemStack,
    /// The last [`proto::Health`] sent, so only a change goes out.
    sent: Option<proto::Health>,
}

impl Survival {
    pub fn new() -> Survival {
        Survival {
            health: MAX_HEALTH,
            max_health: MAX_HEALTH,
            absorption: 0.0,
            effects: items::potion::Effects::new(),
            food: FoodStats::default(),
            air: AIR_MAX,
            fall_distance: 0.0,
            fire: 0,
            in_water: false,
            hurt_resistant: 0,
            last_damage: 0.0,
            respawn_invulnerable: RESPAWN_INVULNERABLE,
            ticks_existed: 0,
            dead: false,
            xp: proto::Xp::default(),
            xp_seed: crate::config::random_u64() as i32,
            xp_cooldown: 0,
            last_xp_sound: 0,
            death_time: 0,
            inv: InventoryPlayer::new(),
            combat: Combat::default(),
            ticks_since_last_swing: 0,
            main_hand: ItemStack::EMPTY,
            sent: None,
        }
    }

    /// `setHealth`
    pub fn set_health(&mut self, h: f32) {
        self.health = h.clamp(0.0, self.max_health);
    }

    fn health_msg(&self) -> proto::Health {
        proto::Health {
            health: self.health,
            food: self.food.level.clamp(0, 255) as u8,
            saturation: self.food.saturation,
            air: self.air.clamp(i16::MIN as i32, i16::MAX as i32) as i16,
            flags: if self.fire > 0 { proto::HEALTH_BURNING } else { 0 },
            absorption: self.absorption,
        }
    }
}

pub struct State {
    pub players: HashMap<u32, Survival>,
    /// The players' shared `Entity.rand`.
    pub rand: JavaRandom,
}

impl Default for State {
    fn default() -> State {
        State { players: HashMap::new(), rand: JavaRandom::new(crate::config::random_u64() as i64) }
    }
}

/// `capabilities.disableDamage`: creative and spectator.
pub fn disable_damage(mode: u8) -> bool {
    mode == proto::GM_CREATIVE || mode == proto::GM_SPECTATOR
}

/// `capabilities.allowFlying`: creative and spectator.
pub fn allow_flying(mode: u8) -> bool {
    disable_damage(mode)
}

pub fn mode(game: &Game, id: u32) -> u8 {
    game.sessions.get(&id).map_or(proto::GM_SURVIVAL, |s| s.mode)
}

pub fn get(game: &Game, id: u32) -> Option<&Survival> {
    game.players.players.get(&id)
}

pub fn difficulty(game: &Game) -> Difficulty {
    Difficulty::parse(&game.cfg.difficulty)
}

/// `Entity.getEyeHeight` for a player: 1.62, 0.08 lower when sneaking.
pub fn eye_height(sneaking: bool) -> f32 {
    if sneaking {
        1.62 - 0.08
    } else {
        1.62
    }
}

/// The player's bounding box standing at `p` (`setSize(0.6F, 1.8F)`, `Entity.setPosition`).
pub fn player_bb(p: [f64; 3]) -> Aabb {
    let f = (0.6f32 / 2.0) as f64;
    let h = 1.8f32 as f64;
    Aabb::new(p[0] - f, p[1], p[2] - f, p[0] + f, p[1] + h, p[2] + f)
}

pub fn on_join(game: &mut Game, id: u32) {
    game.players.players.insert(id, Survival::new());
    playerdata::apply(game, id);
    if game.sessions.get(&id).is_some_and(|s| s.survival_client) {
        send_all(game, id);
    }
}

pub fn on_leave(game: &mut Game, id: u32) {
    playerdata::write(game, id);
    game.players.players.remove(&id);
}

pub fn on_frame(game: &mut Game, id: u32, tag: u8, _body: &[u8]) -> bool {
    // The survival client says hello after the welcome: it gets its bars now.
    if tag == proto::V_CLIENT_INFO && game.sessions.get(&id).is_some_and(|s| s.survival_client) {
        send_all(game, id);
    }
    false
}

/// Health and experience, as a joining or respawning client needs them.
pub fn send_all(game: &mut Game, id: u32) {
    let Some(p) = game.players.players.get_mut(&id) else { return };
    p.sent = None;
    let xp = p.xp;
    net::send(game, id, proto::S_XP, &xp.encode());
    sync(game, id);
    super::effects::send_all(game, id);
    send_slots(game, id);
}

/// `SPacketWindowItems` for the window on screen and the cursor: the 46 `ContainerPlayer` slots
/// (0 result, 1 to 4 grid, 5 to 8 armor head to feet, 9 to 35 main, 36 to 44 hotbar, 45 off hand),
/// or a crafting table's 46 (windows.rs). Survival clients only.
pub fn send_slots(game: &mut Game, id: u32) {
    super::windows::send_full(game, id);
}

/// A teleport (`/tp`, `/spawn`): the fall so far is forgotten and the next state the viewer sends
/// is the baseline at the new place, so the jump is not read as a drop (`Entity.fallDistance`
/// reset by `NetHandlerPlayServer.setPlayerLocation`'s `updateLastPosition`).
pub fn on_teleport(game: &mut Game, id: u32) {
    if let Some(p) = game.players.players.get_mut(&id) {
        p.fall_distance = 0.0;
    }
    if let Some(s) = game.sessions.get_mut(&id) {
        s.state = None;
    }
}

/// `EntityPlayerMP.onUpdateEntity`'s `SPacketUpdateHealth`, when something it carries changed.
pub fn sync(game: &mut Game, id: u32) {
    if !game.sessions.get(&id).is_some_and(|s| s.survival_client) {
        return;
    }
    let Some(p) = game.players.players.get_mut(&id) else { return };
    let msg = p.health_msg();
    if p.sent == Some(msg) {
        return;
    }
    p.sent = Some(msg);
    net::send(game, id, proto::S_HEALTH, &msg.encode());
}

/// `EntityLivingBase.heal`
pub fn heal(game: &mut Game, id: u32, amount: f32) {
    if let Some(p) = game.players.players.get_mut(&id) {
        if p.health > 0.0 {
            p.set_health(p.health + amount);
        }
    }
}

/// `EntityPlayer.addExhaustion`
pub fn add_exhaustion(game: &mut Game, id: u32, amount: f32) {
    if disable_damage(mode(game, id)) {
        return;
    }
    if let Some(p) = game.players.players.get_mut(&id) {
        p.food.add_exhaustion(amount);
    }
}

/// A throwaway `java.util.Random` for the Unbreaking roll of `ItemStack.damageItem`, which the
/// callers cannot take from `game` while they hold a stack borrowed out of it.
pub fn wear_rand() -> JavaRandom {
    JavaRandom::new(crate::config::random_u64() as i64)
}

/// `Entity.setFire`, with the fire protection of `EntityLivingBase.setFire` through the armor.
pub fn set_fire(game: &mut Game, id: u32, seconds: i32) {
    if let Some(p) = game.players.players.get_mut(&id) {
        let lvl = items::enchant::get_max_enchantment_level(items::enchant::id::FIRE_PROTECTION, p.inv.armor.iter());
        let ticks = items::enchant::fire_time_for_entity(lvl, seconds * 20);
        p.fire = p.fire.max(ticks);
    }
}

pub fn on_game_mode(game: &mut Game, id: u32, mode: u8) {
    // `EntityPlayer.onUpdate`: a player who cannot be hurt stops burning.
    if let Some(p) = game.players.players.get_mut(&id) {
        if disable_damage(mode) {
            p.fire = 0;
        }
    }
    sync(game, id);
}

// ---- what the player stands in ----

/// The block state at `at`, on the game's terrain.
fn state_at(game: &Game, at: (i32, i32, i32)) -> u16 {
    terrain::state(game, at)
}

/// `Entity.isInsideOfMaterial(Material.WATER)` for a player with feet at `p`.
pub fn eye_in_water(game: &Game, p: [f64; 3], sneaking: bool) -> bool {
    let d0 = p[1] + eye_height(sneaking) as f64;
    let at = (floor(p[0]), floor(d0), floor(p[2]));
    let s = state_at(game, at);
    if props(s).mat != Mat::Water {
        return false;
    }
    let f = liquid_height_percent((s & 15) as i32) - 0.111_111_11;
    let f1 = (at.1 + 1) as f32 - f;
    d0 < f1 as f64
}

/// `Entity.handleMaterialAcceleration` for water, the box `handleWaterMovement` asks about.
fn in_water_bb(game: &Game, p: [f64; 3]) -> bool {
    let bb = player_bb(p).grow(0.0, -0.400_000_005_960_464_5, 0.0).shrink(0.001);
    handle_material_acceleration(&Blocks(game), &bb, Mat::Water)
}

/// `Entity.isInLava`
fn in_lava(game: &Game, p: [f64; 3]) -> bool {
    let bb = player_bb(p).grow(-0.100_000_001_490_116_12, -0.400_000_005_960_464_5, -0.100_000_001_490_116_12);
    is_material_in_bb(&Blocks(game), &bb, Mat::Lava)
}

/// The block id `EntityLivingBase.isOnLadder` stands in: a ladder or vine at the feet.
/// DROPPED: the trapdoor that opens onto a ladder below it (`canGoThroughtTrapDoorOnLadder`).
pub fn ladder_block(game: &Game, p: [f64; 3], mode: u8) -> Option<u16> {
    if mode == proto::GM_SPECTATOR {
        return None;
    }
    let id = state_at(game, (floor(p[0]), floor(p[1]), floor(p[2]))) >> 4;
    (id == LADDER || id == VINE).then_some(id)
}

/// `Entity.isEntityInsideOpaqueBlock`, with the eye height and the eight corners it samples.
/// DROPPED: `Block.causesSuffocation` per block; `isNormalCube` stands in for it.
fn inside_opaque_block(game: &Game, p: [f64; 3], sneaking: bool) -> bool {
    let eye = eye_height(sneaking);
    let mut last = None;
    for i in 0..8i32 {
        let j = floor(p[1] + (((i >> 0) % 2) as f32 - 0.5) as f64 * 0.1 + eye as f64);
        let k = floor(p[0] + ((((i >> 1) % 2) as f32 - 0.5) * 0.6 * 0.8) as f64);
        let l = floor(p[2] + ((((i >> 2) % 2) as f32 - 0.5) * 0.6 * 0.8) as f64);
        if last != Some((k, j, l)) {
            last = Some((k, j, l));
            if is_normal_cube(state_at(game, (k, j, l))) {
                return true;
            }
        }
    }
    false
}

// ---- the tick ----

pub fn tick(game: &mut Game) {
    let mut ids: Vec<u32> = game.players.players.keys().copied().collect();
    ids.sort_unstable();
    for id in ids {
        tick_player(game, id);
        sync(game, id);
    }
}

/// `EntityPlayerMP.onUpdate`, from `onEntityUpdate` to `FoodStats.onUpdate`.
fn tick_player(game: &mut Game, id: u32) {
    let Some(st) = game.sessions.get(&id).and_then(|s| s.state) else { return };
    let mode = mode(game, id);
    let disable = disable_damage(mode);
    let pos = st.pos;
    let sneaking = st.flags & SNEAKING != 0;
    {
        let Some(p) = game.players.players.get_mut(&id) else { return };
        if p.dead {
            return;
        }
        p.ticks_existed += 1;
        // EntityPlayer.onUpdate: the orb pickup cooldown.
        if p.xp_cooldown > 0 {
            p.xp_cooldown -= 1;
        }
        p.respawn_invulnerable -= 1;
        if p.hurt_resistant > 0 {
            p.hurt_resistant -= 1;
        }
        // EntityPlayer.onUpdate: the swing counter, and a different item in hand restarts it.
        p.ticks_since_last_swing += 1;
        let held = p.inv.get_current_item();
        if !items::stack::are_item_stacks_equal(&p.main_hand, &held) {
            if !items::stack::are_items_equal_ignore_durability(&p.main_hand, &held) {
                p.ticks_since_last_swing = 0;
            }
            p.main_hand = held;
        }
    }
    // Entity.onEntityUpdate: handleWaterMovement, fire, lava, the void.
    let wet = in_water_bb(game, pos);
    let lava = in_lava(game, pos);
    {
        let p = game.players.players.get_mut(&id).unwrap();
        p.in_water = wet;
        if wet {
            p.fall_distance = 0.0;
            p.fire = 0;
        }
    }
    let fire = get(game, id).map_or(0, |p| p.fire);
    if fire > 0 {
        if fire % 20 == 0 {
            damage::attack_entity_from(game, id, &DamageSource::on_fire(), 1.0);
        }
        if let Some(p) = game.players.players.get_mut(&id) {
            p.fire -= 1;
        }
    }
    if lava {
        // setOnFireFromLava
        damage::attack_entity_from(game, id, &DamageSource::lava(), 4.0);
        set_fire(game, id, 15);
        if let Some(p) = game.players.players.get_mut(&id) {
            p.fall_distance *= 0.5;
        }
    }
    if pos[1] < -64.0 {
        // EntityLivingBase.outOfWorld
        damage::attack_entity_from(game, id, &DamageSource::out_of_world(), 4.0);
    }
    // EntityLivingBase.onEntityUpdate
    let alive = |game: &Game| get(game, id).is_some_and(|p| p.health > 0.0);
    if alive(game) && !(mode == proto::GM_SPECTATOR) && inside_opaque_block(game, pos, sneaking) {
        damage::attack_entity_from(game, id, &DamageSource::in_wall(), 1.0);
    }
    if alive(game) {
        if !eye_in_water(game, pos, sneaking) {
            game.players.players.get_mut(&id).unwrap().air = AIR_MAX;
        } else if !disable && !get(game, id).is_some_and(|p| p.effects.is_active(items::potion::Effect::WaterBreathing)) {
            // decreaseAirSupply: Respiration of the helmet skips the loss with chance level / (level + 1).
            let air = {
                let p = game.players.players.get_mut(&id).unwrap();
                let i = items::enchant::get_enchantment_level(items::enchant::id::RESPIRATION, &p.inv.armor[3]);
                if !(i > 0 && game.players.rand.next_int_bounded(i + 1) > 0) {
                    p.air -= 1;
                }
                p.air
            };
            if air == -20 {
                game.players.players.get_mut(&id).unwrap().air = 0;
                damage::attack_entity_from(game, id, &DamageSource::drown(), 2.0);
            }
        }
    }
    if alive(game) {
        // isWet: DROPPED, rain.
        let p = game.players.players.get_mut(&id).unwrap();
        if p.in_water {
            p.fire = 0;
        }
    }
    // EntityPlayer.onUpdate: a player who cannot be hurt does not burn.
    if disable {
        game.players.players.get_mut(&id).unwrap().fire = 0;
    }
    // EntityLivingBase.onEntityUpdate: updatePotionEffects.
    super::effects::tick(game, id);
    // EntityPlayer.onLivingUpdate
    if difficulty(game) == Difficulty::Peaceful && terrain::rule(game, "naturalRegeneration", true) {
        let p = game.players.players.get_mut(&id).unwrap();
        if p.health < p.max_health && p.ticks_existed % 20 == 0 {
            p.set_health(p.health + 1.0);
        }
        if p.food.need_food() && p.ticks_existed % 10 == 0 {
            p.food.level += 1;
        }
    }
    // EntityLivingBase.travel: a ladder or vine ends the fall (not in liquid).
    if !wet && !lava && ladder_block(game, pos, mode).is_some() {
        game.players.players.get_mut(&id).unwrap().fall_distance = 0.0;
    }
    // EntityPlayer.onUpdate: foodStats.onUpdate
    let (diff, regen) = (difficulty(game), terrain::rule(game, "naturalRegeneration", true));
    let tick = {
        let p = game.players.players.get_mut(&id).unwrap();
        p.food.on_update(diff, regen, p.health, p.max_health)
    };
    if tick.heal > 0.0 {
        heal(game, id, tick.heal);
    }
    if tick.starve {
        damage::attack_entity_from(game, id, &DamageSource::starve(), 1.0);
    }
}

// ---- movement ----

/// `Math.round(MathHelper.sqrt(d) * 100.0F)`
fn round_cm(d: f64) -> i32 {
    let v = (d.sqrt() as f32) * 100.0;
    (v + 0.5).floor() as i32
}

/// A session's new state, before it replaces `game.sessions[id].state`, so the old one is there
/// for the movement deltas. The first state after a join or a respawn is only the baseline.
pub fn on_state(game: &mut Game, id: u32, new: &PlayerState) {
    let Some(old) = game.sessions.get(&id).and_then(|s| s.state) else { return };
    let mode = mode(game, id);
    if get(game, id).is_none_or(|p| p.dead) {
        return;
    }
    let (dx, dy, dz) = (new.pos[0] - old.pos[0], new.pos[1] - old.pos[1], new.pos[2] - old.pos[2]);
    let on_ground = new.flags & ON_GROUND != 0;
    let sprinting = new.flags & SPRINTING != 0;
    let sneaking = new.flags & SNEAKING != 0;
    // `if (this.player.onGround && !packetIn.isOnGround() && d8 > 0.0D) this.player.jump()`, then
    // EntityPlayer.jump's exhaustion.
    if old.flags & ON_GROUND != 0 && !on_ground && dy > 0.0 {
        add_exhaustion(game, id, if sprinting { 0.2 } else { 0.05 });
    }
    // Entity.move's doBlockCollisions: BlockWeb.onEntityCollidedWithBlock -> setInWeb.
    if in_cobweb(game, new.pos) {
        game.players.players.get_mut(&id).unwrap().fall_distance = 0.0;
    }
    // EntityPlayer.addMovementStat
    let in_water = get(game, id).is_some_and(|p| p.in_water);
    if eye_in_water(game, new.pos, sneaking) {
        let i = round_cm(dx * dx + dy * dy + dz * dz);
        if i > 0 {
            add_exhaustion(game, id, 0.01 * i as f32 * 0.01);
        }
    } else if in_water {
        let j = round_cm(dx * dx + dz * dz);
        if j > 0 {
            add_exhaustion(game, id, 0.01 * j as f32 * 0.01);
        }
    } else if ladder_block(game, new.pos, mode).is_some() {
        // CLIMB_ONE_CM is a statistic only.
    } else if on_ground {
        let k = round_cm(dx * dx + dz * dz);
        if k > 0 && sprinting {
            add_exhaustion(game, id, 0.1 * k as f32 * 0.01);
        }
        // Walking and sneaking add `0.0F * k * 0.01F`.
    }
    // EntityPlayerMP.handleFalling(posY - d3, onGround)
    handle_falling(game, id, new.pos, dy, on_ground, sneaking);
}

fn in_cobweb(game: &Game, p: [f64; 3]) -> bool {
    let bb = player_bb(p).shrink(0.001);
    for x in floor(bb.min_x)..=floor(bb.max_x) {
        for y in floor(bb.min_y)..=floor(bb.max_y) {
            for z in floor(bb.min_z)..=floor(bb.max_z) {
                if state_at(game, (x, y, z)) >> 4 == COBWEB {
                    return true;
                }
            }
        }
    }
    false
}

/// `EntityPlayerMP.handleFalling`, then `Entity.updateFallState`.
fn handle_falling(game: &mut Game, id: u32, p: [f64; 3], y: f64, on_ground: bool, sneaking: bool) {
    let at = (floor(p[0]), floor(p[1] - 0.200_000_002_980_232_24), floor(p[2]));
    let mut block = state_at(game, at) >> 4;
    if block == 0 {
        let below = state_at(game, (at.0, at.1 - 1, at.2)) >> 4;
        let is_fence = matches!(below, FENCE | NETHER_BRICK_FENCE | 188..=192);
        let is_gate = matches!(below, FENCE_GATE | 183..=187);
        if is_fence || below == COBBLESTONE_WALL || is_gate {
            block = below;
        }
    }
    let Some(player) = game.players.players.get_mut(&id) else { return };
    if on_ground {
        let distance = player.fall_distance;
        if distance > 0.0 {
            if block == FARMLAND {
                super::blockticks::farmland_fallen_upon(game, at, distance);
            }
            on_fallen_upon(game, id, block, distance, sneaking);
        }
        game.players.players.get_mut(&id).unwrap().fall_distance = 0.0;
    } else if y < 0.0 {
        player.fall_distance = (player.fall_distance as f64 - y) as f32;
    }
}

/// `Block.onFallenUpon` for the blocks that override it (the farmland's trampling is
/// `blockticks::farmland_fallen_upon`, called before this).
fn on_fallen_upon(game: &mut Game, id: u32, block: u16, distance: f32, sneaking: bool) {
    let multiplier = match block {
        HAY => 0.2,
        BED => 0.5,
        // BlockSlime: a sneaking player lands as on any block, otherwise the fall is 0.
        SLIME if !sneaking => 0.0,
        FARMLAND => 1.0,
        _ => 1.0,
    };
    fall(game, id, distance, multiplier);
}

/// `EntityLivingBase.ceil(float)`: `MathHelper.ceil`.
fn ceil_f32(v: f32) -> i32 {
    let i = v as i32;
    if v > i as f32 {
        i + 1
    } else {
        i
    }
}

/// `EntityPlayer.fall`, then `EntityLivingBase.fall`.
fn fall(game: &mut Game, id: u32, distance: f32, damage_multiplier: f32) {
    if allow_flying(mode(game, id)) {
        return;
    }
    let jump = game.players.players.get(&id).map_or(0.0, |p| p.effects.jump_boost_level());
    let i = ceil_f32((distance - 3.0 - jump) * damage_multiplier);
    if i > 0 {
        let Some(st) = game.sessions.get(&id).and_then(|s| s.state) else { return };
        // playSound(getFallSound(i)), 16 blocks around. DROPPED: the landing block's own fall sound.
        let name = if i > 4 { "entity.player.big_fall" } else { "entity.player.small_fall" };
        let sound = proto::Sound { name: name.into(), category: 7, pos: [st.pos[0] as f32, st.pos[1] as f32, st.pos[2] as f32], volume: 1.0, pitch: 1.0 };
        net::send_near(game, None, st.pos, 16.0, proto::S_SOUND, &sound.encode());
        damage::attack_entity_from(game, id, &DamageSource::fall(), i as f32);
    }
}

// ---- dropped items ----

/// `EntityPlayer.dropItem(stack, dropAround, traceItem)`. The entity goes to item_entities
/// (items.rs).
pub fn drop_item(game: &mut Game, id: u32, stack: ItemStack, drop_around: bool, trace: bool) {
    if stack.is_empty() {
        return;
    }
    let Some(st) = game.sessions.get(&id).and_then(|s| s.state) else { return };
    let name = game.sessions[&id].name.clone();
    let rand = &mut game.players.rand;
    let d0 = st.pos[1] - 0.300_000_011_920_928_96 + eye_height(st.flags & SNEAKING != 0) as f64;
    let mut e = sim::item_entity::ItemEntity::new(st.pos[0], d0, st.pos[2], stack, JavaRandom::new(rand.next_long()));
    e.set_pickup_delay(40);
    if trace {
        e.thrower = Some(name);
    }
    if drop_around {
        let f = rand.next_float() * 0.5;
        let f1 = rand.next_float() * 6.283_185_5;
        e.body.motion.x = (-sin(f1) * f) as f64;
        e.body.motion.z = (cos(f1) * f) as f64;
        e.body.motion.y = 0.200_000_002_980_232_24;
    } else {
        let (yaw, pitch) = (st.yaw, st.pitch);
        let mut f2 = 0.3f32;
        e.body.motion.x = (-sin(yaw * 0.017_453_292) * cos(pitch * 0.017_453_292) * f2) as f64;
        e.body.motion.z = (cos(yaw * 0.017_453_292) * cos(pitch * 0.017_453_292) * f2) as f64;
        e.body.motion.y = (-sin(pitch * 0.017_453_292) * f2 + 0.1) as f64;
        let f3 = rand.next_float() * 6.283_185_5;
        f2 = 0.02 * rand.next_float();
        e.body.motion.x += (f3 as f64).cos() * f2 as f64;
        e.body.motion.y += ((rand.next_float() - rand.next_float()) * 0.1) as f64;
        e.body.motion.z += (f3 as f64).sin() * f2 as f64;
    }
    item_entities::spawn_stack(game, e);
}

/// `InventoryPlayer.dropAllItems`: every stack on the ground around the player.
pub fn drop_inventory(game: &mut Game, id: u32) {
    let stacks = match game.players.players.get_mut(&id) {
        Some(p) => p.inv.drop_all_items(),
        None => return,
    };
    for s in stacks {
        drop_item(game, id, s, true, false);
    }
}

pub fn save(_game: &Game) -> Value {
    Value::Null
}

pub fn load(_game: &mut Game, _v: Value) {}
