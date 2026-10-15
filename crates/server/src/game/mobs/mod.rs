//! Mobs: the `Mob` struct, the per-kind table, the tick (`EntityLivingBase.onUpdate`,
//! `onLivingUpdate`, `travel` through `sim::entity`), `/summon`, save and load, and the `Tracked`
//! view the entity tracker reads (range 80, interval 3, velocity on, from `sim::kinds`). No AI
//! yet (T9b): a mob falls, collides and floats as vanilla's
//! `EntityLiving` does with every AI task idle. Combat (T9c) is `combat.rs` (the player's attack,
//! `attackEntityFrom`, knockback, death, a mob's melee on a player) and `loot.rs` (loot tables). Natural spawning and despawning (T9d) are `spawner.rs` (with
//! `spawn_lists.rs`, the creature types and biome lists).
//!
//! Mobs live in one list in spawn order. Entities in unloaded chunks are not ticked. A mob that
//! is dead is removed at the end of its tick and the tracker sends `S_DESTROY` when it next runs.

use glam::DVec3;
use proto::{MobMeta, SpawnMob};
use serde_json::{json, Value};
use sim::collision::{floor, BlockAccess};
use path::node::{PathNodeType, Priorities};
use path::{EntityJumpHelper, EntityLookHelper, EntityMoveHelper, NavEntity, PathNavigateGround};
use sim::entity::{travel_living, Body, Living};
use sim::collision::Aabb;
use items::ItemStack;
use sim::kinds::{self, Kind};

use worldgen::java_random::JavaRandom;

use super::terrain::Blocks;
use super::tracker::Tracked;
use super::{net, Game};

pub mod ai;
pub mod block_fx;
pub mod animals;
pub mod arrow;
pub mod combat;
pub mod creeper;
pub mod effects;
pub mod interact;
pub mod loot;
pub mod path;
pub mod spawn_lists;
pub mod skeleton;
pub mod sounds;
pub mod spawner;
pub mod zombie;

/// What `EntityLivingBase` takes from each kind's `applyEntityAttributes`.
#[derive(Clone, Copy, Debug)]
pub struct MobDef {
    pub kind: &'static Kind,
    /// `SharedMonsterAttributes.MAX_HEALTH`
    pub max_health: f32,
    /// `SharedMonsterAttributes.MOVEMENT_SPEED`, which `getAIMoveSpeed` returns.
    pub speed: f32,
    /// `SharedMonsterAttributes.FOLLOW_RANGE`: `EntityLiving` registers 16, `EntityZombie` sets 35.
    pub follow_range: f32,
    /// `SharedMonsterAttributes.ATTACK_DAMAGE` for a kind that registers it (`EntityMob` does,
    /// with base 2.0; the animals do not).
    pub attack_damage: Option<f32>,
    /// `SharedMonsterAttributes.ARMOR` (`getTotalArmorValue`; no mob carries armor items yet).
    pub armor: f32,
}

/// Every kind that can be a mob. Adding a mob is one entry here (and its kind in `sim::kinds`).
pub static MOBS: [MobDef; 8] = [
    // EntityPig.applyEntityAttributes: 10 health, 0.25 speed.
    MobDef { kind: &kinds::PIG, max_health: 10.0, speed: 0.25, follow_range: 16.0, attack_damage: None, armor: 0.0 },
    // EntitySheep: 8, 0.23.
    MobDef { kind: &kinds::SHEEP, max_health: 8.0, speed: 0.23, follow_range: 16.0, attack_damage: None, armor: 0.0 },
    // EntityCow: 10, 0.2.
    MobDef { kind: &kinds::COW, max_health: 10.0, speed: 0.2, follow_range: 16.0, attack_damage: None, armor: 0.0 },
    // EntityChicken: 4, 0.25.
    MobDef { kind: &kinds::CHICKEN, max_health: 4.0, speed: 0.25, follow_range: 16.0, attack_damage: None, armor: 0.0 },
    // EntityZombie: 20, follow range 35, attack 3, armor 2, 0.23.
    MobDef { kind: &kinds::ZOMBIE, max_health: 20.0, speed: 0.23, follow_range: 35.0, attack_damage: Some(3.0), armor: 2.0 },
    // EntitySkeleton: 20, 0.25.
    MobDef { kind: &kinds::SKELETON, max_health: 20.0, speed: 0.25, follow_range: 16.0, attack_damage: Some(2.0), armor: 0.0 },
    // EntityCreeper: 20, 0.25. No attack damage: attackEntityAsMob returns true and hurts nothing.
    MobDef { kind: &kinds::CREEPER, max_health: 20.0, speed: 0.25, follow_range: 16.0, attack_damage: None, armor: 0.0 },
    // EntitySpider: 16, 0.3.
    MobDef { kind: &kinds::SPIDER, max_health: 16.0, speed: 0.3, follow_range: 16.0, attack_damage: Some(2.0), armor: 0.0 },
];

/// The mob definition for a registry name (`minecraft:` optional), if it is one.
pub fn def_by_name(name: &str) -> Option<&'static MobDef> {
    let k = kinds::by_name(name)?;
    MOBS.iter().find(|d| d.kind.name == k.name)
}

pub fn def_of(kind: &Kind) -> Option<&'static MobDef> {
    MOBS.iter().find(|d| d.kind.name == kind.name)
}

/// One living mob (`EntityLiving`).
#[derive(Clone, Debug)]
pub struct Mob {
    pub id: u32,
    pub def: &'static MobDef,
    pub body: Body,
    /// `rotationYawHead`
    pub head_yaw: f32,
    pub health: f32,
    /// `isDead`
    pub dead: bool,
    pub meta: MobMeta,
    /// `Entity.isAirBorne`: set by a knockback, cleared after the tracker's tick.
    pub air_borne: bool,
    /// `Entity.velocityChanged`
    pub velocity_changed: bool,
    /// `moveStrafing`, `moveForward` (the move helper sets them).
    pub move_strafing: f32,
    pub move_forward: f32,
    /// `EntityLivingBase.isJumping`, set by the jump helper.
    pub is_jumping: bool,
    /// `EntityLivingBase.jumpTicks`
    pub jump_ticks: i32,
    /// `EntityLivingBase.landMovementFactor` (`getAIMoveSpeed`): the move helper sets it.
    pub ai_move_speed: f32,
    /// `EntityLivingBase.renderYawOffset`. DROPPED: `EntityBodyHelper`'s stationary smoothing;
    /// it follows `rotationYaw` while the mob moves.
    pub render_yaw_offset: f32,
    /// `EntityLiving.mapPathPriority`
    pub path_priority: Priorities,
    /// `EntityLiving.navigator`, `moveHelper`, `jumpHelper`, `lookHelper`
    pub navigator: PathNavigateGround,
    pub move_helper: EntityMoveHelper,
    pub jump_helper: EntityJumpHelper,
    pub look_helper: EntityLookHelper,
    /// `Entity.hurtResistantTime`
    pub hurt_resistant: i32,
    /// `EntityLivingBase.lastDamage`
    pub last_damage: f32,
    /// `EntityLivingBase.hurtTime`
    pub hurt_time: i32,
    /// `EntityLivingBase.deathTime`
    pub death_time: i32,
    /// `onDeathUpdate` reached tick 20 with `recentlyHit > 0`: the mod tick drops the experience.
    pub award_xp: bool,
    /// `EntityLivingBase.recentlyHit`
    pub recently_hit: i32,
    /// `EntityLivingBase.attackingPlayer`: the session id of the last player to hurt it.
    pub attacking_player: Option<u32>,
    /// `EntityLiving.tasks` (the goal selector) and `targetTasks`.
    pub tasks: ai::Tasks,
    pub target_tasks: ai::Tasks,
    /// `EntityLiving.attackTarget`: an entity id (a session id for a player).
    pub attack_target: Option<u32>,
    /// `Entity.fire`, in ticks. `MM_BURNING` in the meta flags follows it.
    pub fire: i32,
    /// `EntityZombie` `ARMS_RAISED`; on the wire as `MM_AGGRESSIVE`.
    pub arms_raised: bool,
    /// `AbstractSkeleton` `SWINGING_ARMS` (`isSwingingArms`); on the wire as `MM_AGGRESSIVE`.
    pub swinging_arms: bool,
    /// `getHeldItemMainhand`, and what the trackers were last told (`EntityLivingBase.onUpdate`).
    pub main_hand: ItemStack,
    pub sent_main_hand: ItemStack,
    /// `getItemStackFromSlot(HEAD)`: the helmet a burning-in-daylight mob wears down instead of
    /// catching fire. DROPPED: it is not sent to the trackers (no armour layer on the model).
    pub head: ItemStack,
    /// `isHandActive` and `activeItemStack`'s `itemInUseCount`, for the main hand.
    pub hand_active: bool,
    pub item_in_use_count: i32,
    /// `World.getDifficulty().getId()`, refreshed each tick for `getMaxFallHeight`.
    pub difficulty_id: i32,
    /// `Entity.ticksExisted`
    pub ticks_existed: i32,
    /// `EntityLivingBase.revengeTarget` (an entity id) and `revengeTimer`.
    pub revenge_target: Option<u32>,
    pub revenge_timer: i32,
    /// `EntityAgeable.growingAge`, `forcedAge`
    pub growing_age: i32,
    pub forced_age: i32,
    /// `EntityAnimal.inLove` and `playerInLove` (a session id; vanilla stores the UUID).
    pub in_love: i32,
    pub love_cause: Option<u32>,
    /// `EntityLiving.idleTime`: ticks since a player was within 32 blocks (reset by `despawnEntity`).
    pub idle_time: i32,
    /// `EntityLiving.persistenceRequired`: exempt from despawning and from the spawn caps.
    pub persistent: bool,
    /// `EntitySheep` `DYE_COLOR`: the fleece colour metadata in bits 0 to 3, sheared in bit 4.
    pub dye_color: u8,
    /// `EntityPig` `SADDLED`
    pub saddled: bool,
    /// `EntityChicken.timeUntilNextEgg`
    pub egg_timer: i32,
    /// `EntityLiving.livingSoundTime`
    pub living_sound_time: i32,
    /// Attribute multipliers an addon applies on top of the kind's `MobDef` (all 1.0 for a vanilla
    /// mob).
    pub attrs: Attrs,
    /// `EntityCreeper`: the fuse and radius (idle on other kinds).
    pub creeper: creeper::Creeper,
    /// `EntityLivingBase.activePotionsMap`, the absorption amount, and the changes the trackers
    /// have not been told yet.
    pub effects: items::potion::Effects,
    pub absorption: f32,
    pub effect_events: Vec<effects::FxEvent>,
}

/// A mob's attribute multipliers and follow range, as an addon leaves them. The defaults leave the
/// `MobDef` in charge, so a mob no addon touches behaves exactly as vanilla.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Attrs {
    /// `MAX_HEALTH` multiplier.
    pub mod_health: f64,
    /// `MOVEMENT_SPEED` multiplier.
    pub mod_speed: f64,
    /// `ATTACK_DAMAGE` multiplier.
    pub mod_damage: f64,
    /// `FOLLOW_RANGE`.
    pub follow_range: f32,
}

impl Default for Attrs {
    fn default() -> Attrs {
        Attrs { mod_health: 1.0, mod_speed: 1.0, mod_damage: 1.0, follow_range: 0.0 }
    }
}

impl Living for Mob {
    fn body(&mut self) -> &mut Body {
        &mut self.body
    }

    fn move_self(&mut self, w: &impl BlockAccess, x: f64, y: f64, z: f64) {
        self.body.move_entity(w, x, y, z, false, true);
    }
}

impl Mob {
    /// `new EntityX(world)` then `setLocationAndAngles(x, y, z, yaw, 0)`.
    pub fn new(id: u32, def: &'static MobDef, pos: [f64; 3], yaw: f32) -> Mob {
        let mut body = Body::new(DVec3::new(pos[0], pos[1], pos[2]), yaw, 0.0);
        body.width = def.kind.width;
        body.height = def.kind.height;
        body.set_position(body.pos);
        let mut path_priority = Priorities::default();
        // EntityChicken's constructor: setPathPriority(WATER, 0.0F).
        if def.kind.name == "chicken" {
            path_priority.set(PathNodeType::Water, 0.0);
        }
        let mut mob = Mob {
            id,
            def,
            body,
            head_yaw: yaw,
            health: def.max_health,
            dead: false,
            meta: MobMeta::default(),
            air_borne: false,
            velocity_changed: false,
            move_strafing: 0.0,
            move_forward: 0.0,
            is_jumping: false,
            jump_ticks: 0,
            ai_move_speed: 0.0,
            render_yaw_offset: yaw,
            path_priority,
            navigator: PathNavigateGround::new(),
            move_helper: EntityMoveHelper::default(),
            jump_helper: EntityJumpHelper::default(),
            look_helper: EntityLookHelper::default(),
            hurt_resistant: 0,
            last_damage: 0.0,
            hurt_time: 0,
            death_time: 0,
            award_xp: false,
            recently_hit: 0,
            attacking_player: None,
            idle_time: 0,
            persistent: false,
            tasks: ai::Tasks::new(),
            target_tasks: ai::Tasks::new(),
            attack_target: None,
            fire: 0,
            arms_raised: false,
            swinging_arms: false,
            main_hand: ItemStack::EMPTY,
            sent_main_hand: ItemStack::EMPTY,
            head: ItemStack::EMPTY,
            hand_active: false,
            item_in_use_count: 0,
            difficulty_id: 2,
            ticks_existed: 0,
            revenge_target: None,
            revenge_timer: 0,
            growing_age: 0,
            forced_age: 0,
            in_love: 0,
            love_cause: None,
            dye_color: 0,
            saddled: false,
            egg_timer: 0,
            living_sound_time: 0,
            creeper: creeper::Creeper::default(),
            effects: items::potion::Effects::new(),
            absorption: 0.0,
            effect_events: Vec::new(),
            attrs: Attrs { follow_range: def.follow_range, ..Default::default() },
        };
        mob.init_entity_ai();
        if mob.is_creeper() {
            mob.init_creeper_ai();
        }
        mob
    }

    /// `getEntityAttribute(MAX_HEALTH).getAttributeValue()`: the `MobDef` health with an addon's
    /// multiplier. The damage clamp in `combat::damage_entity` still reads the kind's own maximum.
    pub fn max_health(&self) -> f32 {
        self.def.max_health * self.attrs.mod_health as f32 + self.effects.max_health_bonus() as f32
    }

    /// `getEntityAttribute(MOVEMENT_SPEED).getAttributeValue()`
    pub fn movement_speed(&self) -> f32 {
        self.def.speed * self.attrs.mod_speed as f32 * self.effects.movement_speed_factor() as f32
    }

    /// `getEntityAttribute(ATTACK_DAMAGE).getAttributeValue()`
    pub fn attack_damage(&self) -> Option<f32> {
        self.def.attack_damage.map(|d| d * self.attrs.mod_damage as f32 + self.effects.attack_damage_bonus() as f32)
    }

    /// `getEntityAttribute(FOLLOW_RANGE).getAttributeValue()`
    pub fn follow_range(&self) -> f32 {
        self.attrs.follow_range
    }

    /// Whether the kind is an `EntityAnimal`.
    pub fn is_animal(&self) -> bool {
        matches!(self.def.kind.name, "pig" | "sheep" | "cow" | "chicken")
    }

    /// `Entity.isBurning`
    pub fn is_burning(&self) -> bool {
        self.meta.flags & proto::MM_BURNING != 0
    }

    pub fn get_growing_age(&self) -> i32 {
        self.growing_age
    }

    pub fn is_child(&self) -> bool {
        self.growing_age < 0
    }

    pub fn is_in_love(&self) -> bool {
        self.in_love > 0
    }

    pub fn reset_in_love(&mut self) {
        self.in_love = 0;
    }

    /// `getEyeHeight`: `EntityCow` and `EntityChicken` give the height when small (a chicken
    /// always), `EntitySheep` 0.95 of it, other animals `EntityLivingBase`'s 0.85 of it.
    pub fn eye_height(&self) -> f32 {
        match self.def.kind.name {
            "chicken" => self.body.height,
            "sheep" => 0.95 * self.body.height,
            "cow" if self.is_child() => self.body.height,
            _ if self.is_animal() && self.is_child() => self.body.height * 0.85,
            _ => self.def.kind.eye_height,
        }
    }

    /// `EntityAgeable.setGrowingAge`: the baby flag and the size.
    pub fn set_growing_age(&mut self, age: i32) {
        if age < 0 {
            self.meta.flags |= proto::MM_CHILD;
        } else {
            self.meta.flags &= !proto::MM_CHILD;
        }
        self.growing_age = age;
        self.set_scale_for_age(age < 0);
    }

    /// `EntityAgeable.setScaleForAge` into `Entity.setSize`. DROPPED: the `move` that follows a
    /// widening box; the box is centred on the position instead.
    fn set_scale_for_age(&mut self, child: bool) {
        let s = if child { 0.5 } else { 1.0 };
        let (w, h) = (self.def.kind.width * s, self.def.kind.height * s);
        let b = &mut self.body;
        if w != b.width || h != b.height {
            b.width = w;
            b.height = h;
            let d = w as f64 / 2.0;
            b.bb = Aabb::new(b.pos.x - d, b.pos.y, b.pos.z - d, b.pos.x + d, b.pos.y + h as f64, b.pos.z + d);
        }
    }

    /// `EntityAgeable.ageUp`
    pub fn age_up(&mut self, growth_seconds: i32, update_forced_age: bool) {
        let mut i = self.growing_age;
        let j = i;
        i += growth_seconds * 20;
        if i > 0 {
            i = 0;
        }
        let k = i - j;
        self.set_growing_age(i);
        if update_forced_age {
            self.forced_age += k;
        }
        if self.growing_age == 0 {
            self.set_growing_age(self.forced_age);
        }
    }

    /// `EntityAnimal.canMateWith`
    pub fn can_mate_with(&self, other: &Mob) -> bool {
        other.id != self.id && other.def.kind.name == self.def.kind.name && self.is_in_love() && other.is_in_love()
    }

    /// `EntityAnimal.getBlockPathWeight`: 10 over grass, else the light brightness minus 0.5.
    /// DROPPED: the light read; the brightness is taken as full (0.5), so ground that is not
    /// grass ties.
    pub fn get_block_path_weight(&self, w: &impl BlockAccess, pos: (i32, i32, i32)) -> f32 {
        if !self.is_animal() {
            return 0.0;
        }
        if w.state(pos.0, pos.1 - 1, pos.2) >> 4 == worldgen::blocks::id::GRASS {
            10.0
        } else {
            0.5
        }
    }

    /// `PathNavigate.tryMoveToEntityLiving` for a target standing at `at`.
    pub fn try_move_to_entity_living(&mut self, w: &impl BlockAccess, at: [f64; 3], speed: f64) -> bool {
        let mut e = self.nav_entity(w, true);
        let ok = self.navigator.try_move_to_entity_living(w, &mut e, DVec3::new(at[0], at[1], at[2]), speed);
        self.apply_nav_entity(e);
        ok
    }

    /// The fire part of `Entity.onEntityUpdate`: water puts it out, a burn every 20 ticks
    /// (`dealFireDamage(1)`, `IN_FIRE`), lava (`setOnFireFromLava`), and the burning flag.
    /// DROPPED: `isImmuneToFire` (no ported mob has it), rain, and fire protection.
    fn update_fire(&mut self, w: &Blocks, env: &mut ai::Env) {
        if self.body.in_water {
            self.fire = 0;
        }
        if self.fire > 0 {
            if self.fire % 20 == 0 {
                env.fx.push(ai::Effect::Hurt { id: self.id, source: crate::game::damage::DamageSource::in_fire(), amount: 1.0 });
            }
            self.fire -= 1;
        }
        if self.body.is_in_lava(w) {
            env.fx.push(ai::Effect::Hurt { id: self.id, source: crate::game::damage::DamageSource::lava(), amount: 4.0 });
            self.set_fire(15);
        }
        if self.fire > 0 {
            self.meta.flags |= proto::MM_BURNING;
        } else {
            self.meta.flags &= !proto::MM_BURNING;
        }
    }

    /// `Entity.setFire` (without the fire protection enchantment).
    pub fn set_fire(&mut self, seconds: i32) {
        self.fire = self.fire.max(seconds * 20);
    }

    /// `EntityLivingBase.onUpdate` for a mob on the server: `Entity.onUpdate` (`onEntityUpdate`:
    /// the previous position and angles, water, the void), then `onLivingUpdate`.
    /// DROPPED: fire and lava damage, air and drowning, the hurt window, arrow and stinger
    /// counts, `updateEntityActionState` (the AI, T9b), `collideWithNearbyEntities`, and the
    /// body-yaw smoothing the renderer reads. The hurt window, hurt time, death update and
    /// `recentlyHit` countdown of `onEntityUpdate` run as [`Mob::tick_hurt`].
    pub fn update(&mut self, w: &Blocks, env: &mut ai::Env) {
        // EntityCreeper.onUpdate runs its fuse before EntityLiving.onUpdate.
        self.creeper_pre_update(env.fx);
        self.ticks_existed += 1;
        let b = &mut self.body;
        b.prev_pos = b.pos;
        b.prev_pitch = b.pitch;
        b.prev_yaw = b.yaw;
        b.handle_water_movement(w);
        // A sound left from outside `update` is stale; this tick's steps play after the move.
        b.sounds.clear();
        self.update_fire(w, env);
        // Entity.onEntityUpdate: below -64 the entity leaves the world (outOfWorld).
        if self.body.pos.y < -64.0 {
            self.dead = true;
            return;
        }
        self.tick_hurt();
        self.tick_effects(env);
        // EntityLiving.onEntityUpdate: the living sound.
        self.living_sound_update(env.rand, env.fx);
        // EntityLivingBase.onUpdate: updateActiveHand, then the equipment diff.
        self.update_active_hand();
        if self.main_hand != self.sent_main_hand {
            env.fx.push(ai::Effect::Equipment { id: self.id, slot: proto::EQ_MAIN_HAND, stack: self.main_hand });
            self.sent_main_hand = self.main_hand;
        }
        // EntityLivingBase.onEntityUpdate: the revenge target lapses when dead or after 100 ticks.
        if let Some(t) = self.revenge_target {
            if !env.entity_alive(t) || self.ticks_existed - self.revenge_timer > 100 {
                self.revenge_target = None;
            }
        }
        self.on_living_update(w, env);
        // Entity.move's step sounds.
        self.play_step_sounds(w, env.fx);
        // The arms-raised bit: a zombie's `isArmsRaised`, a skeleton's `isSwingingArms`.
        if self.arms_raised || self.swinging_arms {
            self.meta.flags |= proto::MM_AGGRESSIVE;
        } else {
            self.meta.flags &= !proto::MM_AGGRESSIVE;
        }
    }

    /// `EntityLiving.getMaxFallHeight`: 3, or with an attack target the health above a third of
    /// the maximum less 4 per difficulty step below hard.
    pub fn max_fall_height(&self) -> i32 {
        if self.is_creeper() {
            return self.creeper_max_fall_height();
        }
        if self.attack_target.is_none() {
            return 3;
        }
        let mut i = (self.health - self.max_health() * 0.33) as i32;
        i -= (3 - self.difficulty_id) * 4;
        i.max(0) + 3
    }

    /// `PathNavigate.getPathToEntityLiving` for a target standing at `at`.
    pub fn get_path_to_entity_living(&mut self, w: &impl BlockAccess, at: [f64; 3]) -> Option<path::point::Path> {
        let mut e = self.nav_entity(w, true);
        let p = self.navigator.get_path_to_entity_living(w, &mut e, DVec3::new(at[0], at[1], at[2]));
        self.apply_nav_entity(e);
        p
    }

    /// `PathNavigateGround.canNavigate`: on the ground, or swimming.
    pub fn can_navigate(&self, w: &impl BlockAccess) -> bool {
        let e = self.nav_entity(w, true);
        self.navigator.can_navigate(&e)
    }

    /// `PathNavigate.setPath`
    pub fn set_path(&mut self, w: &impl BlockAccess, p: Option<path::point::Path>, speed: f64) -> bool {
        let e = self.nav_entity(w, true);
        self.navigator.set_path(w, &e, p, speed)
    }

    /// The `EntityLiving` fields the navigator, move helper and look helper read and write.
    pub fn nav_entity(&self, w: &impl BlockAccess, with_lava: bool) -> NavEntity {
        let b = &self.body;
        NavEntity {
            pos: b.pos,
            bb: b.bb,
            width: b.width,
            height: b.height,
            eye_height: self.eye_height(),
            step_height: b.step_height,
            on_ground: b.on_ground,
            in_water: b.in_water,
            in_lava: with_lava && b.is_in_lava(w),
            max_fall_height: self.max_fall_height(),
            follow_range: self.follow_range(),
            movement_speed: self.movement_speed(),
            priorities: self.path_priority,
            yaw: b.yaw,
            pitch: b.pitch,
            head_yaw: self.head_yaw,
            render_yaw_offset: self.render_yaw_offset,
            ai_move_speed: self.ai_move_speed,
            move_forward: self.move_forward,
            move_strafing: self.move_strafing,
            is_jumping: self.is_jumping,
        }
    }

    /// Writes back what the helpers changed.
    fn apply_nav_entity(&mut self, e: NavEntity) {
        self.body.yaw = e.yaw;
        self.body.pitch = e.pitch;
        self.head_yaw = e.head_yaw;
        self.path_priority = e.priorities;
        self.ai_move_speed = e.ai_move_speed;
        self.move_forward = e.move_forward;
        self.move_strafing = e.move_strafing;
        self.is_jumping = e.is_jumping;
    }

    /// `PathNavigate.tryMoveToXYZ` on this mob's navigator: the entry point a goal calls.
    pub fn try_move_to(&mut self, w: &impl BlockAccess, x: f64, y: f64, z: f64, speed: f64) -> bool {
        let mut e = self.nav_entity(w, true);
        let ok = self.navigator.try_move_to_xyz(w, &mut e, x, y, z, speed);
        self.apply_nav_entity(e);
        ok
    }

    /// `EntityLiving.updateEntityActionState` after the despawn check (which `tick` runs):
    /// the navigator, then the move, look and jump helpers.
    /// DROPPED: `updateAITasks`, riding.
    fn update_entity_action_state(&mut self, w: &Blocks, env: &mut ai::Env) {
        // targetTasks, then tasks.
        let mut target_tasks = std::mem::take(&mut self.target_tasks);
        let mut tasks = std::mem::take(&mut self.tasks);
        {
            let mut c = ai::Ctx { w, mob: self, others: env.others.reborrow(), players: env.players, rand: env.rand, fx: env.fx, sky_subtracted: env.sky_subtracted };
            target_tasks.on_update_tasks(&mut c);
            tasks.on_update_tasks(&mut c);
        }
        self.target_tasks = target_tasks;
        self.tasks = tasks;
        let mut e = self.nav_entity(w, !self.navigator.no_path());
        self.navigator.on_update_navigation(w, &mut e, &mut self.move_helper);
        self.apply_nav_entity(e);
        // EntityAnimal.updateAITasks
        if self.is_animal() && self.growing_age != 0 {
            self.in_love = 0;
        }
        let mut e = self.nav_entity(w, !self.navigator.no_path());
        self.move_helper.on_update_move_helper(w, &mut e, &mut self.jump_helper, self.navigator.get_node_processor());
        self.look_helper.on_update_look(&mut e, !self.navigator.no_path());
        self.jump_helper.do_jump(&mut e);
        self.apply_nav_entity(e);
    }

    /// `EntityAgeable.onLivingUpdate` (the server branch) then `EntityAnimal.onLivingUpdate`.
    /// DROPPED: the heart particles every 10 ticks of love (the server's `spawnParticle` sends
    /// nothing), and the client forced-age particles.
    fn animal_living_update(&mut self) {
        let mut i = self.growing_age;
        if i < 0 {
            i += 1;
            self.set_growing_age(i);
        } else if i > 0 {
            i -= 1;
            self.set_growing_age(i);
        }
        if self.growing_age != 0 {
            self.in_love = 0;
        }
        if self.in_love > 0 {
            self.in_love -= 1;
        }
    }

    /// `EntityLivingBase.jump`: no jump boost, not sprinting.
    fn jump(&mut self) {
        self.body.motion.y = 0.42f32 as f64;
        self.air_borne = true;
    }

    /// `EntityLivingBase.onLivingUpdate`, the server branch.
    /// DROPPED: `collideWithNearbyEntities`, elytra, the client interpolation.
    fn on_living_update(&mut self, w: &Blocks, env: &mut ai::Env) {
        if self.jump_ticks > 0 {
            self.jump_ticks -= 1;
        }
        for m in [&mut self.body.motion.x, &mut self.body.motion.y, &mut self.body.motion.z] {
            if m.abs() < 0.003 {
                *m = 0.0;
            }
        }
        // isMovementBlocked is health <= 0.
        let before = (self.body.pos.x, self.body.pos.z);
        if self.health <= 0.0 {
            self.is_jumping = false;
            self.move_strafing = 0.0;
            self.move_forward = 0.0;
        } else {
            self.update_entity_action_state(w, env);
        }
        if self.is_jumping {
            if self.body.in_water || self.body.is_in_lava(w) {
                // handleJumpWater, handleJumpLava
                self.body.motion.y += 0.039_999_999_105_930_79;
            } else if self.body.on_ground && self.jump_ticks == 0 {
                self.jump();
                self.jump_ticks = 10;
            }
        } else {
            self.jump_ticks = 0;
        }
        self.move_strafing *= 0.98;
        self.move_forward *= 0.98;
        let (strafe, forward) = (self.move_strafing, self.move_forward);
        let speed = self.ai_move_speed;
        // jumpMovementFactor is 0.02 for everything but a player.
        travel_living(self, w, strafe, 0.0, forward, false, speed, 0.02);
        if (self.body.pos.x, self.body.pos.z) != before {
            self.render_yaw_offset = self.body.yaw;
        }
        if self.is_animal() {
            self.animal_living_update();
            self.chicken_living_update(env);
        }
    }
}

pub struct State {
    /// Every mob, in spawn order.
    pub mobs: Vec<Mob>,
    /// Every arrow in flight or stuck in a block (`arrow.rs`).
    pub arrows: Vec<arrow::Arrow>,
    /// The mobs' shared `Entity.rand`: knockback, loot tables. DROPPED: one generator per entity.
    pub rand: JavaRandom,
    /// Parsed loot tables by path, `None` for one the jar lacks.
    pub loot: std::collections::HashMap<String, Option<Value>>,
    /// The spawner's generators and the chunks that had their world-generation animals.
    pub spawner: spawner::State,
}

impl Default for State {
    fn default() -> State {
        State { mobs: Vec::new(), arrows: Vec::new(), rand: JavaRandom::new(crate::config::random_u64() as i64), loot: Default::default(), spawner: Default::default() }
    }
}

/// `World.spawnEntity` for a mob: an id, and the tracker sends `S_SPAWN_MOB` on its next run.
pub fn spawn(game: &mut Game, def: &'static MobDef, pos: [f64; 3], yaw: f32) -> u32 {
    let id = net::new_entity_id(game);
    let mut m = Mob::new(id, def, pos, yaw);
    // Difficulty is read by `setCombatTask` on spawn, before the first tick refreshes it.
    m.difficulty_id = super::players::difficulty(game) as i32;
    m.on_construct(&mut game.mobs.rand);
    // The addons' join hooks: the task pass first, then the roll that needs the generator.
    crate::game::addons::mob_init(game, &mut m);
    crate::game::addons::on_spawn(game, &mut m);
    game.mobs.mobs.push(m);
    game.stats.mobs = game.mobs.mobs.len();
    id
}

/// [`spawn`] then `onInitialSpawn`, which the spawner and `/summon` call and a baby does not.
pub fn spawn_initial(game: &mut Game, def: &'static MobDef, pos: [f64; 3], yaw: f32) -> u32 {
    let id = spawn(game, def, pos, yaw);
    let st = &mut game.mobs;
    if let Some(m) = st.mobs.last_mut() {
        m.on_initial_spawn(&mut st.rand);
    }
    id
}

/// `/summon <kind> [x y z]`: the mob where it was asked, if that chunk is loaded
/// (`commands.summon.outOfWorld`, whose feedback the command cannot give from here).
pub fn summon(game: &mut Game, kind: &'static Kind, pos: [f64; 3]) -> Option<u32> {
    let def = def_of(kind)?;
    if !Blocks(game).loaded(floor(pos[0]), floor(pos[2])) {
        return None;
    }
    Some(spawn_initial(game, def, pos, 0.0))
}

pub fn on_join(_game: &mut Game, _id: u32) {}

pub fn on_leave(_game: &mut Game, _id: u32) {}

/// `V_ATTACK`: `NetHandlerPlayServer.processUseEntity` ATTACK.
pub fn on_frame(game: &mut Game, id: u32, tag: u8, body: &[u8]) -> bool {
    match tag {
        proto::V_ATTACK => {
            if let Some(a) = proto::Attack::decode(body) {
                combat::process_attack(game, id, a.target);
            }
            true
        }
        proto::V_INTERACT => {
            if let Some(a) = proto::Interact::decode(body) {
                interact::process_interact(game, id, a.target, a.hand);
            }
            true
        }
        _ => false,
    }
}

/// `World.updateEntities` for the mobs: each one in a loaded chunk ticks, the dead go.
pub fn tick(game: &mut Game) {
    let mut mobs = std::mem::take(&mut game.mobs.mobs);
    let mut rand = std::mem::replace(&mut game.mobs.rand, JavaRandom::new(0));
    let views = ai::player_views(game);
    let mut fx: Vec<ai::Effect> = Vec::new();
    let players = spawner::player_positions(game);
    let difficulty = super::players::difficulty(game);
    let peaceful = difficulty == super::food::Difficulty::Peaceful;
    let difficulty_id = difficulty as i32;
    let sky = spawner::sky_subtracted(game);
    // EntityLivingBase.onDeathUpdate at tick 20: (position, experience) to drop as orbs.
    let mut awards: Vec<([f64; 3], i32)> = Vec::new();
    let do_mob_loot = super::terrain::rule(game, "doMobLoot", true);
    for i in 0..mobs.len() {
        let (before, rest) = mobs.split_at_mut(i);
        let (m, after) = rest.split_first_mut().expect("in range");
        if m.dead {
            continue;
        }
        if !Blocks(game).loaded(floor(m.body.pos.x), floor(m.body.pos.z)) {
            continue;
        }
        // EntityLivingBase.onLivingUpdate -> EntityLiving.updateEntityActionState, which runs
        // unless isMovementBlocked (health <= 0). DROPPED: the rest of it is the AI (T9b).
        if m.health > 0.0 {
            m.idle_time += 1;
            let near = spawner::nearest_player_sq(&players, [m.body.pos.x, m.body.pos.y, m.body.pos.z]);
            let can = spawner::can_despawn(m.def);
            if spawner::despawn_entity(m.persistent, &mut m.idle_time, near, can, &mut rand) {
                m.dead = true;
                continue;
            }
        }
        m.difficulty_id = difficulty_id;
        if m.health > 0.0 {
            zombie::pre_update(game, m, &mut rand, sky);
        }
        let mut env = ai::Env { others: ai::Others { before, after }, players: &views, rand: &mut rand, fx: &mut fx, sky_subtracted: sky };
        m.update(&Blocks(game), &mut env);
        if m.award_xp {
            m.award_xp = false;
            if do_mob_loot {
                let p = m.body.pos;
                awards.push(([p.x, p.y, p.z], m.experience_points(&mut rand)));
            }
        }
        // EntityMob.onUpdate: a hostile mob does not survive peaceful.
        if peaceful && spawn_lists::is_hostile(m.def) {
            m.dead = true;
        }
    }
    // EntityCreeper.explode: the creeper is dead (not saved, no loot) and the blast goes off.
    let blasts: Vec<([f64; 3], f32, u32)> = mobs
        .iter()
        .filter(|m| m.creeper.exploding)
        .map(|m| ([m.body.pos.x, m.body.pos.y, m.body.pos.z], m.explosion_size(), m.id))
        .collect();
    // The effect changes of this tick go to the trackers.
    let mut told: Vec<(u32, effects::FxEvent)> = Vec::new();
    for m in &mut mobs {
        told.extend(m.effect_events.drain(..).map(|e| (m.id, e)));
    }
    mobs.retain(|m| !m.dead);
    game.mobs.mobs = mobs;
    game.mobs.rand = rand;
    game.stats.mobs = game.mobs.mobs.len();
    for (pos, xp) in awards {
        super::xp_orbs::spawn_orbs(game, pos, xp);
    }
    apply_effects(game, fx);
    send_effect_events(game, told);
    arrow::tick(game);
    for (pos, size, id) in blasts {
        let griefing = super::terrain::rule(game, "mobGriefing", true);
        let exploder = crate::game::damage::Attacker { name: "Creeper".into(), pos, player: false, id: Some(id) };
        super::explosion::create_explosion(game, Some(exploder), pos, size, griefing);
    }
}

/// `SPacketEntityEffect` and `SPacketRemoveEntityEffect` to the players tracking each mob.
pub fn send_effect_events(game: &mut Game, told: Vec<(u32, effects::FxEvent)>) {
    for (id, e) in told {
        match e {
            effects::FxEvent::Set(p) => {
                let body = crate::game::effects::effect_msg(id, &p).encode();
                super::tracker::send_to_tracking(game, id, proto::S_EFFECT, &body);
            }
            effects::FxEvent::Gone(e) => {
                let body = proto::RemoveEffect { id, effect: e.id() as u8 }.encode();
                super::tracker::send_to_tracking(game, id, proto::S_REMOVE_EFFECT, &body);
            }
        }
    }
}

/// What only the game can do, collected during the mob tick: babies, drops, block changes and
/// entity statuses.
pub fn apply_effects(game: &mut Game, fx: Vec<ai::Effect>) {
    for f in fx {
        match f {
            // EntityAIMate.spawnBaby: setGrowingAge(-24000) and the spawn at the parent.
            ai::Effect::Child { def, pos, dyes } => {
                let id = spawn(game, def, pos, 0.0);
                // EntityAIMate.spawnBaby: with doMobLoot, an orb of 1 + nextInt(7) at the parent.
                if super::terrain::rule(game, "doMobLoot", true) {
                    let xp = 1 + game.mobs.rand.next_int_bounded(7);
                    super::xp_orbs::spawn_orbs(game, pos, xp);
                }
                // EntitySheep.createChild: getDyeColorMixFromParents.
                let color = dyes.map(|(i, j)| ai::sheep_dye_mix(game, i, j));
                if let Some(m) = game.mobs.mobs.iter_mut().find(|m| m.id == id) {
                    m.set_growing_age(-24000);
                    if let Some(c) = color {
                        m.set_fleece_color(c);
                    }
                }
            }
            // EntityChicken.dropItem(EGG, 1): entityDropItem(stack, 0).
            ai::Effect::Drop { pos, stack } => {
                super::items::spawn_stack_at(game, pos, stack, None, sim::item_entity::DEFAULT_PICKUP_DELAY);
            }
            ai::Effect::SetBlock { at, state } => {
                super::terrain::set_block(game, at, state);
            }
            // The pillar's step up: the block, then the mob on top of it, so the block is there for
            // the mob's next `move`.
            ai::Effect::StepUp { mob, at, state, to } => {
                super::terrain::set_block(game, at, state);
                if let Some(m) = game.mobs.mobs.iter_mut().find(|m| m.id == mob) {
                    m.body.set_position(glam::DVec3::new(to[0], to[1], to[2]));
                }
            }
            ai::Effect::Break { at, state, drop } => {
                block_fx::destroy_block(game, at, state, drop);
            }
            ai::Effect::Harvest { at, state, stack } => {
                block_fx::harvest(game, at, state, &stack);
            }
            ai::Effect::BreakAnim { breaker, at, stage } => {
                block_fx::send_progress(game, breaker, at, stage);
            }
            ai::Effect::Attack { mob, target } => {
                combat::attack_entity_as_mob(game, mob, target);
            }
            ai::Effect::Hurt { id, source, amount } => {
                combat::hurt_mob(game, id, &source, amount);
            }
            ai::Effect::Arrow(a) => {
                arrow::spawn(game, a);
            }
            ai::Effect::Equipment { id, slot, stack } => {
                let body = proto::Equipment { id, slot, stack: super::items::wire_stack(&stack) }.encode();
                super::tracker::send_to_tracking(game, id, proto::S_EQUIPMENT, &body);
            }
            ai::Effect::Sound { pos, name, category, volume, pitch } => {
                sounds::play_sound(game, pos, &name, category, volume, pitch);
            }
            ai::Effect::Event { id, status } => {
                super::tracker::send_to_tracking(game, id, proto::S_ENTITY_EVENT, &proto::EntityEvent { id, status }.encode());
            }
        }
    }
}

/// The tracker has run: `Entity.isAirBorne` and `velocityChanged` are cleared.
pub fn after_tracker(game: &mut Game) {
    for m in &mut game.mobs.mobs {
        m.air_borne = false;
        m.velocity_changed = false;
    }
}

/// What the tracker reads off one mob.
pub struct View<'a>(pub &'a Mob);

pub fn views(state: &State) -> Vec<View<'_>> {
    state.mobs.iter().map(View).collect()
}

impl Tracked for View<'_> {
    fn id(&self) -> u32 {
        self.0.id
    }
    fn kind(&self) -> &'static Kind {
        self.0.def.kind
    }
    fn pos(&self) -> [f64; 3] {
        let p = self.0.body.pos;
        [p.x, p.y, p.z]
    }
    fn rotation(&self) -> [f32; 3] {
        [self.0.body.yaw, self.0.body.pitch, self.0.head_yaw]
    }
    fn on_ground(&self) -> bool {
        self.0.body.on_ground
    }
    fn motion(&self) -> [f64; 3] {
        let m = self.0.body.motion;
        [m.x, m.y, m.z]
    }
    fn spawn_body(&self) -> Vec<u8> {
        let (b, m) = (&self.0.body, self.0.body.motion);
        SpawnMob {
            id: self.0.id,
            kind: self.0.def.kind.id.unwrap_or(0),
            pos: [b.pos.x, b.pos.y, b.pos.z],
            yaw: b.yaw,
            pitch: b.pitch,
            head_yaw: self.0.head_yaw,
            // SPacketSpawnMob clamps the motion to 3.9 a tick.
            motion: [m.x, m.y, m.z].map(|v| v.clamp(-3.9, 3.9) as f32),
            meta: self.0.meta,
        }
        .encode()
    }
    fn meta_body(&self) -> Option<Vec<u8>> {
        Some(proto::EntityMeta { id: self.0.id, payload: proto::MetaPayload::Mob(self.0.meta) }.encode())
    }
    fn equipment(&self) -> Vec<(u8, proto::WireStack)> {
        if self.0.main_hand.is_empty() {
            return Vec::new();
        }
        vec![(proto::EQ_MAIN_HAND, super::items::wire_stack(&self.0.main_hand))]
    }
    fn air_borne(&self) -> bool {
        self.0.air_borne
    }
    fn velocity_changed(&self) -> bool {
        self.0.velocity_changed
    }
}

/// `mobs.json`: every mob (`writeEntityToNBT` and the living base: id, Pos, Motion, Rotation,
/// Health, DeathTime, OnGround). Entity ids are not saved; they are handed out again on load.
pub fn save(game: &Game) -> Value {
    let list: Vec<Value> = game
        .mobs
        .mobs
        .iter()
        .filter(|m| !m.dead)
        .map(|m| {
            let (p, v) = (m.body.pos, m.body.motion);
            json!({
                "id": m.def.kind.name,
                "pos": [p.x, p.y, p.z],
                "motion": [v.x, v.y, v.z],
                "yaw": m.body.yaw,
                "pitch": m.body.pitch,
                "head_yaw": m.head_yaw,
                "health": m.health,
                "death_time": m.death_time,
                "on_ground": m.body.on_ground,
                "flags": m.meta.flags,
                "persistence_required": m.persistent,
                "age": m.growing_age,
                "forced_age": m.forced_age,
                "in_love": m.in_love,
                // EntitySheep: Sheared, Color. EntityPig: Saddle. EntityChicken: EggLayTime.
                "sheared": m.get_sheared(),
                "color": m.get_fleece_color(),
                "saddle": m.saddled,
                "egg_lay_time": m.egg_timer,
                "fire": m.fire,
                "main_hand": m.main_hand.get_item(),
                "head": if m.head.is_empty() { Value::Null } else { json!([m.head.get_item(), m.head.get_metadata()]) },
                "absorption": m.absorption,
                "effects": crate::game::effects::effects_json(&m.effects),
            })
        })
        .collect();
    let mut done: Vec<[i32; 2]> = game.mobs.spawner.worldgen_done.iter().map(|&(x, z)| [x, z]).collect();
    done.sort_unstable();
    json!({"version": 1, "mobs": list, "worldgen_spawned": done})
}

pub fn load(game: &mut Game, v: Value) {
    if let Some(done) = v.get("worldgen_spawned").and_then(Value::as_array) {
        for c in done {
            if let (Some(x), Some(z)) = (c.get(0).and_then(Value::as_i64), c.get(1).and_then(Value::as_i64)) {
                game.mobs.spawner.worldgen_done.insert((x as i32, z as i32));
            }
        }
    }
    let Some(list) = v.get("mobs").and_then(Value::as_array) else { return };
    for it in list {
        let Some(def) = it.get("id").and_then(Value::as_str).and_then(def_by_name) else { continue };
        let f = |key: &str, i: usize| it.get(key).and_then(Value::as_array).and_then(|a| a.get(i)).and_then(Value::as_f64).unwrap_or(0.0);
        let num = |key: &str, d: f64| it.get(key).and_then(Value::as_f64).unwrap_or(d);
        let id = spawn(game, def, [f("pos", 0), f("pos", 1), f("pos", 2)], num("yaw", 0.0) as f32);
        let m = game.mobs.mobs.last_mut().expect("just spawned");
        debug_assert_eq!(m.id, id);
        (m.body.motion.x, m.body.motion.y, m.body.motion.z) = (f("motion", 0), f("motion", 1), f("motion", 2));
        m.body.pitch = num("pitch", 0.0) as f32;
        m.head_yaw = num("head_yaw", m.body.yaw as f64) as f32;
        // The kind's health with Epic Siege's multiplier, which `spawn` already applied.
        m.health = num("health", m.max_health() as f64) as f32;
        m.death_time = num("death_time", 0.0) as i32;
        m.body.on_ground = it.get("on_ground").and_then(Value::as_bool).unwrap_or(false);
        m.meta.flags = num("flags", 0.0) as u8;
        m.persistent = it.get("persistence_required").and_then(Value::as_bool).unwrap_or(false);
        // EntityAgeable.readEntityFromNBT: Age, ForcedAge; EntityAnimal: InLove.
        m.set_growing_age(num("age", 0.0) as i32);
        m.forced_age = num("forced_age", 0.0) as i32;
        m.in_love = num("in_love", 0.0) as i32;
        m.set_sheared(it.get("sheared").and_then(Value::as_bool).unwrap_or(false));
        m.set_fleece_color(num("color", 0.0) as u8);
        m.saddled = it.get("saddle").and_then(Value::as_bool).unwrap_or(false);
        m.fire = num("fire", 0.0) as i32;
        crate::game::effects::read_effects(&mut m.effects, it.get("effects"));
        m.absorption = num("absorption", 0.0).max(0.0) as f32;
        if m.fire > 0 {
            m.meta.flags |= proto::MM_BURNING;
        }
        // AbstractSkeleton.readEntityFromNBT: the combat task follows the hand. The spawn hooks'
        // own roll gives way to what was saved.
        m.main_hand = ItemStack::EMPTY;
        if let Some(i) = it.get("main_hand").and_then(Value::as_u64).filter(|&i| i != 0) {
            m.set_item_in_main_hand(ItemStack::of(i as u16));
        }
        m.head = ItemStack::EMPTY;
        if let Some([id, damage]) = it.get("head").and_then(Value::as_array).map(|a| [0, 1].map(|i| a.get(i).and_then(Value::as_i64).unwrap_or(0))) {
            if id != 0 {
                m.head = ItemStack::of(id as u16);
                m.head.set_item_damage(damage as i32);
            }
        }
        // readEntityFromNBT: EggLayTime only when present, else the constructor's draw stands.
        if let Some(t) = it.get("egg_lay_time").and_then(Value::as_i64) {
            m.egg_timer = t as i32;
        }
    }
}

/// The spawning step of the tick (`WorldEntitySpawner.findChunksForSpawning`), after the mobs tick.
pub fn spawn_step(game: &mut Game) {
    spawner::step(game);
    crate::game::addons::tick(game);
}

#[cfg(test)]
#[path = "skeleton_tests.rs"]
mod skeleton_tests;
#[cfg(test)]
#[path = "creeper_tests.rs"]
mod creeper_tests;

#[cfg(test)]
#[path = "tests.rs"]
mod tests;

#[cfg(test)]
#[path = "ai_tests.rs"]
mod ai_tests;

#[cfg(test)]
#[path = "animals_tests.rs"]
mod animals_tests;

#[cfg(test)]
#[path = "zombie_tests.rs"]
mod zombie_tests;

#[cfg(test)]
#[path = "sounds_tests.rs"]
mod sounds_tests;
