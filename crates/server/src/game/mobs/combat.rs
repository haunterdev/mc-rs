//! Mob combat (T9c): a player's attack on an entity (`NetHandlerPlayServer.processUseEntity`
//! ATTACK into `EntityPlayer.attackTargetEntityWithCurrentItem`), `EntityLivingBase.attackEntityFrom`
//! and `knockBack` for a mob, hurt and death (`onDeath`, `onDeathUpdate`, the entity status
//! packets) and a mob's melee on a player (`EntityMob.attackEntityAsMob`, which lands on the T4
//! path in damage.rs). Loot is loot.rs.
//!
//! DROPPED across the file: sounds and particles; enchantments (sharpness-style modifiers,
//! knockback, fire aspect, looting, thorns), armor items, shields and absorption on a mob, the sweep attack (it needs `distanceWalkedModified`, which this server does not
//! integrate), the sprint cancel and the 0.6 motion damping an attack gives the attacker (the
//! viewer owns the player's motion), experience orbs from `onDeathUpdate`, `attackedAtYaw` (the
//! hurt tilt is the viewer's from the status), the animal `attackEntityFrom` override that clears
//! love mode, the statistics and the advancement triggers, and attacks on other players (pvp melee).

use items::tools;
use proto::{ANIM_CRIT, EV_DEATH, EV_HURT, EV_HURT_BURN, EV_HURT_DROWN, SNEAKING, SPRINTING};
use worldgen::java_random::JavaRandom;
use worldgen::math_helper::{cos, sin};

use glam::DVec3;
use sim::raytrace::ray_trace_blocks;

use super::{loot, Mob};
use crate::game::damage::{self, Attacker, DamageSource};
use crate::game::terrain::Blocks;
use crate::game::{net, players, tracker, Game};

/// `Entity.maxHurtResistantTime` of `EntityLivingBase`.
pub const MAX_HURT_RESISTANT: i32 = 20;
/// `EntityLivingBase.maxHurtTime` after a hurt.
pub const MAX_HURT_TIME: i32 = 10;
/// `deathTime` at which `onDeathUpdate` removes the entity.
pub const DEATH_TICKS: i32 = 20;
/// `processUseEntity`: the squared reach to an entity in view, and behind a block.
const REACH_SQ: f64 = 36.0;
const REACH_SQ_BLIND: f64 = 9.0;

/// What one `attackEntityFrom` did to a mob.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Hurt {
    /// The return value: the damage landed.
    pub landed: bool,
    /// The status byte `world.setEntityState` sends (a fresh hit, not one inside the window).
    pub status: Option<u8>,
    /// Health reached 0 on this hit.
    pub died: bool,
}

impl Mob {
    /// `EntityLiving.getExperiencePoints` with the `EntityAnimal` and `EntityZombie` overrides.
    /// DROPPED: the armor loop (no mob wears armor items yet) and the off hand (empty).
    pub fn experience_points(&self, rand: &mut JavaRandom) -> i32 {
        if self.is_animal() {
            // EntityAnimal.getExperiencePoints: 1 + world.rand.nextInt(3).
            return 1 + rand.next_int_bounded(3);
        }
        let mut value = sim::xp_orb::experience_value(self.def.kind.name);
        if self.def.kind.name == "zombie" && self.is_child() {
            value = (value as f32 * 2.5) as i32;
        }
        if value > 0 {
            // An item in a hand with a drop chance of 0.085 (at most 1) adds 1 + nextInt(3).
            if !self.main_hand.is_empty() {
                value += 1 + rand.next_int_bounded(3);
            }
        }
        value
    }

    /// The `onEntityUpdate` part of `EntityLivingBase` that follows the water and void checks:
    /// `hurtTime`, `hurtResistantTime`, `onDeathUpdate` and the `recentlyHit` countdown.
    pub fn tick_hurt(&mut self) {
        if self.hurt_time > 0 {
            self.hurt_time -= 1;
        }
        if self.hurt_resistant > 0 {
            self.hurt_resistant -= 1;
        }
        if self.health <= 0.0 {
            self.on_death_update();
        }
        if self.recently_hit > 0 {
            self.recently_hit -= 1;
        } else {
            self.attacking_player = None;
        }
    }

    /// `EntityLivingBase.onDeathUpdate`: after 20 ticks the experience drops (`award_xp`, spawned
    /// by `mobs::tick`, which has the world) and the entity is removed.
    /// DROPPED: the smoke particles.
    fn on_death_update(&mut self) {
        self.death_time += 1;
        if self.death_time == DEATH_TICKS {
            self.award_xp = self.recently_hit > 0;
            self.dead = true;
        }
    }

    /// `EntityLivingBase.knockBack(entity, strength, xRatio, zRatio)`; the knockback resistance
    /// attribute is 0 for every kind here, so the `rand.nextDouble()` draw always passes.
    pub fn knock_back(&mut self, strength: f32, x_ratio: f64, z_ratio: f64, rand: &mut JavaRandom) {
        if rand.next_double() >= 0.0 {
            self.air_borne = true;
            let f = (x_ratio * x_ratio + z_ratio * z_ratio).sqrt() as f32;
            let m = &mut self.body.motion;
            m.x /= 2.0;
            m.z /= 2.0;
            m.x -= x_ratio / f as f64 * strength as f64;
            m.z -= z_ratio / f as f64 * strength as f64;
            if self.body.on_ground {
                m.y /= 2.0;
                m.y += strength as f64;
                if m.y > 0.400_000_005_960_464_5 {
                    m.y = 0.400_000_005_960_464_5;
                }
            }
        }
    }

    /// `EntityLivingBase.attackEntityFrom` up to the death: the hurt window, `damageEntity`,
    /// `recentlyHit` and the knockback from the attacker. The status packets and the death
    /// (`onDeath`) are [`hurt_mob`]'s, in Java's order.
    pub fn attack_entity_from(&mut self, src: &DamageSource, amount: f32, rand: &mut JavaRandom) -> Hurt {
        if self.health <= 0.0 {
            return Hurt::default();
        }
        // EntityLivingBase: fire damage does nothing to a mob with Fire Resistance.
        if src.fire && self.effects.is_active(items::potion::Effect::FireResistance) {
            return Hurt::default();
        }
        // EntityAnimal.attackEntityFrom: a hit ends love mode.
        self.in_love = 0;
        let mut fresh = true;
        if self.hurt_resistant as f32 > MAX_HURT_RESISTANT as f32 / 2.0 {
            if amount <= self.last_damage {
                return Hurt::default();
            }
            self.damage_entity(src, amount - self.last_damage);
            self.last_damage = amount;
            fresh = false;
        } else {
            self.last_damage = amount;
            self.hurt_resistant = MAX_HURT_RESISTANT;
            self.damage_entity(src, amount);
            self.hurt_time = MAX_HURT_TIME;
        }
        if let Some(a) = &src.attacker {
            // setRevengeTarget(EntityLivingBase attacker)
            self.revenge_target = a.id;
            self.revenge_timer = self.ticks_existed;
            if a.player {
                self.recently_hit = 100;
                self.attacking_player = a.id;
            }
        }
        let mut status = None;
        if fresh {
            status = Some(if src.kind == "drown" {
                EV_HURT_DROWN
            } else if src.fire {
                EV_HURT_BURN
            } else {
                EV_HURT
            });
            if src.kind != "drown" {
                self.velocity_changed = true;
            }
            if let Some(a) = &src.attacker {
                let (mut d1, mut d0) = (a.pos[0] - self.body.pos.x, a.pos[2] - self.body.pos.z);
                while d1 * d1 + d0 * d0 < 1.0e-4 {
                    d1 = (rand.next_double() - rand.next_double()) * 0.01;
                    d0 = (rand.next_double() - rand.next_double()) * 0.01;
                }
                self.knock_back(0.4, d1, d0, rand);
            }
        }
        Hurt { landed: true, status, died: self.health <= 0.0 }
    }

    /// `EntityLivingBase.damageEntity`: `applyArmorCalculations` with the kind's armor attribute
    /// (toughness is 0 for every kind), unless the source bypasses armor.
    /// Then `applyPotionDamageCalculations` (Resistance) and the absorption amount.
    /// DROPPED: armor item wear and the enchantment protection of worn armor (a mob wears none).
    fn damage_entity(&mut self, src: &DamageSource, mut amount: f32) {
        if !src.unblockable {
            amount = damage::damage_after_absorb(amount, self.def.armor, 0.0);
        }
        if !src.absolute && src.kind != "outOfWorld" {
            if let Some(a) = self.effects.amplifier(items::potion::Effect::Resistance) {
                amount = (amount * (25 - (a + 1) * 5) as f32 / 25.0).max(0.0);
            }
        }
        let before = amount;
        amount = (amount - self.absorption).max(0.0);
        self.absorption -= before - amount;
        if amount != 0.0 {
            self.health = (self.health - amount).clamp(0.0, self.max_health().max(self.def.max_health));
        }
    }
}

/// The index of mob `id` in the list.
fn index_of(game: &Game, id: u32) -> Option<usize> {
    game.mobs.mobs.iter().position(|m| m.id == id)
}

/// `attackEntityFrom` on mob `id`, then what follows in Java: the `SPacketEntityStatus` for the
/// hurt, and on death `onDeath` (the loot, then status 3). True when the damage landed.
pub fn hurt_mob(game: &mut Game, id: u32, src: &DamageSource, amount: f32) -> bool {
    let Some(i) = index_of(game, id) else { return false };
    let st = &mut game.mobs;
    let h = st.mobs[i].attack_entity_from(src, amount, &mut st.rand);
    if !h.landed {
        return false;
    }
    if let Some(s) = h.status {
        tracker::send_to_tracking(game, id, proto::S_ENTITY_EVENT, &proto::EntityEvent { id, status: s }.encode());
    }
    if h.died {
        on_death(game, i);
    }
    true
}

/// `EntityLivingBase.onDeath` for the mob at `i`, whose health just hit 0: the loot while
/// `doMobLoot` is on, then `setEntityState(3)`.
fn on_death(game: &mut Game, i: usize) {
    let m = &game.mobs.mobs[i];
    let (id, path, p, saddled) = (m.id, m.loot_table_path(), m.body.pos, m.saddled);
    let killed_by_player = m.recently_hit > 0 && m.attacking_player.is_some();
    let killer = m.attacking_player;
    let looting = match killer {
        Some(k) if killed_by_player => game.players.players.get(&k).map_or(0, |p| items::enchant::get_enchantment_level(items::enchant::id::LOOTING, &p.inv.get_current_item())),
        _ => 0,
    };
    let m = &game.mobs.mobs[i];
    let ctx = loot::Ctx { on_fire: m.fire > 0, killed_by_player, looting };
    loot::drop_loot(game, &path, [p.x, p.y, p.z], ctx);
    tracker::send_to_tracking(game, id, proto::S_ENTITY_EVENT, &proto::EntityEvent { id, status: EV_DEATH }.encode());
    // EntityPig.onDeath: a saddled pig drops the saddle (Entity.dropItem, not gated by doMobLoot).
    if saddled {
        super::super::items::spawn_stack_at(game, [p.x, p.y, p.z], items::ItemStack::of(items::ids::item::SADDLE), None, sim::item_entity::DEFAULT_PICKUP_DELAY);
    }
}

/// The attacker record for a mob (`causeMobDamage`): its display name, e.g. "Zombie".
pub(super) fn mob_attacker(m: &Mob) -> Attacker {
    let n = m.def.kind.name;
    let mut name = n.replace('_', " ");
    if let Some(c) = name.get_mut(0..1) {
        c.make_ascii_uppercase();
    }
    Attacker { name, pos: [m.body.pos.x, m.body.pos.y, m.body.pos.z], player: false, id: Some(m.id) }
}

/// `EntityMob.attackEntityAsMob` on a player: the attribute damage through `causeMobDamage` into
/// the T4 path (`damage::attack_entity_from`: respawn protection, difficulty scaling, the hurt
/// window, knockback, death). This is the entry point melee AI calls. False when the kind has no
/// attack damage, the mob or player is gone, or the damage did not land.
/// DROPPED: the held-item enchantments (no mob holds one), fire aspect, the shield cooldown and
/// the effects a hit adds (a cave spider's poison, a husk's hunger, a wither skeleton's wither):
/// none of those kinds exist in this server.
/// Thorns on the player's armor hurts the mob back (`EnchantmentHelper.applyThornEnchantments`).
pub fn attack_entity_as_mob(game: &mut Game, mob: u32, player: u32) -> bool {
    let Some(i) = index_of(game, mob) else { return false };
    let m = &game.mobs.mobs[i];
    let Some(f) = m.attack_damage() else { return false };
    if m.health <= 0.0 {
        return false;
    }
    let src = DamageSource::mob(mob_attacker(m));
    let landed = damage::attack_entity_from(game, player, &src, f);
    if landed {
        thorns_hit(game, player, mob);
    }
    landed
}

/// `EnchantmentHelper.applyThornEnchantments(user, attacker)`: `getEnchantedItem(THORNS, user)`
/// picks one random armor piece with Thorns; `shouldHit` and `getDamage` draw from the user's
/// random. The attacker takes `causeThornsDamage`, and the piece wears 3 (1 when no damage).
fn thorns_hit(game: &mut Game, player: u32, mob: u32) {
    use items::enchant::{self, id};
    let Some(p) = game.players.players.get_mut(&player) else { return };
    let pieces: Vec<usize> = (0..4).filter(|&k| enchant::get_enchantment_level(id::THORNS, &p.inv.armor[k]) > 0).collect();
    if pieces.is_empty() {
        return;
    }
    let rand = &mut game.players.rand;
    let k = pieces[rand.next_int_bounded(pieces.len() as i32) as usize];
    let level = enchant::get_enchantment_level(id::THORNS, &p.inv.armor[k]);
    let hit = enchant::thorns_should_hit(level, rand);
    let dmg = if hit { enchant::thorns_damage(level, rand) } else { 0 };
    let creative = players::mode(game, player) == proto::GM_CREATIVE;
    let p = game.players.players.get_mut(&player).unwrap();
    // The thorns hurt targets the mob as the player's own `Attacker`.
    let wear = if hit { 3 } else { 1 };
    p.inv.armor[k].damage_item(wear, creative, &mut game.players.rand);
    let name = game.sessions.get(&player).map(|s| s.name.clone()).unwrap_or_default();
    let pos = game.sessions.get(&player).and_then(|s| s.state).map_or([0.0; 3], |s| s.pos);
    if hit {
        let src = DamageSource::thorns(Attacker { name, pos, player: true, id: Some(player) });
        hurt_mob(game, mob, &src, dmg as f32);
    }
    players::send_slots(game, player);
}

/// `NetHandlerPlayServer.processUseEntity` for an ATTACK: the reach check, then
/// `attackTargetEntityWithCurrentItem`. A target that is not a mob (an item, an orb, a player) is
/// ignored: vanilla kicks the client for the first two, and pvp melee is DROPPED.
pub fn process_attack(game: &mut Game, pid: u32, target: u32) {
    let Some(i) = index_of(game, target) else { return };
    let Some(s) = game.sessions.get(&pid) else { return };
    let (Some(st), mode) = (s.state, s.mode) else { return };
    // Spectators have no attack (the client targets for them); a dead player cannot swing.
    if mode == proto::GM_SPECTATOR || game.players.players.get(&pid).is_none_or(|p| p.dead) {
        return;
    }
    let m = &game.mobs.mobs[i];
    let eye = DVec3::new(st.pos[0], st.pos[1] + players::eye_height(st.flags & SNEAKING != 0) as f64, st.pos[2]);
    let to = DVec3::new(m.body.pos.x, m.body.pos.y + m.def.kind.eye_height as f64, m.body.pos.z);
    // canEntityBeSeen: no block on the line between the eyes.
    let seen = ray_trace_blocks(&Blocks(game), eye, to, false, false).is_none();
    let reach = if seen { REACH_SQ } else { REACH_SQ_BLIND };
    let d = (st.pos[0] - m.body.pos.x).powi(2) + (st.pos[1] - m.body.pos.y).powi(2) + (st.pos[2] - m.body.pos.z).powi(2);
    if d < reach {
        attack_target(game, pid, target);
    }
}

/// `world.playSound(null, posX, posY, posZ, sound, getSoundCategory(), 1.0F, 1.0F)` of an attack.
fn attack_sound(game: &Game, pos: [f64; 3], name: &str) {
    super::sounds::play_sound(game, pos, name, super::sounds::PLAYERS, 1.0, 1.0);
}

/// `EntityPlayer.attackTargetEntityWithCurrentItem` on a living mob.
pub fn attack_target(game: &mut Game, pid: u32, target: u32) {
    let Some(s) = game.sessions.get(&pid) else { return };
    let (Some(st), mode, name) = (s.state, s.mode, s.name.clone()) else { return };
    let ladder = players::ladder_block(game, st.pos, mode).is_some();
    // getCreatureAttribute: zombies and skeletons are undead, spiders arthropods.
    let attr = match index_of(game, target).map(|i| game.mobs.mobs[i].def.kind.name) {
        Some("zombie" | "skeleton") => items::enchant::CreatureAttribute::Undead,
        Some("spider") => items::enchant::CreatureAttribute::Arthropod,
        _ => items::enchant::CreatureAttribute::Undefined,
    };
    let Some(p) = game.players.players.get_mut(&pid) else { return };
    // canBeAttackedWithItem, hitByEntity: true and false for every mob.
    let held = p.inv.get_current_item();
    // The attack_damage attribute: base 1.0 plus the held item's modifier (operation 0).
    let (dmg_mod, speed_mod) = if held.is_empty() { (0.0, 0.0) } else { tools::attack_modifiers(held.get_item()).unwrap_or((0.0, 0.0)) };
    // Strength and Weakness are operation 0 modifiers on the attack_damage attribute.
    let mut f = (1.0 + dmg_mod + p.effects.attack_damage_bonus()) as f32;
    let f1 = items::enchant::get_modifier_for_creature(&held, attr);
    let fire_aspect = items::enchant::get_enchantment_level(items::enchant::id::FIRE_ASPECT, &held);
    let kb_level = items::enchant::get_enchantment_level(items::enchant::id::KNOCKBACK, &held);
    // getCooldownPeriod and getCooledAttackStrength(0.5)
    // Haste and Mining Fatigue multiply the attack_speed attribute.
    let period = (1.0 / ((4.0 + speed_mod) * p.effects.attack_speed_factor()) * 20.0) as f32;
    let f2 = ((p.ticks_since_last_swing as f32 + 0.5) / period).clamp(0.0, 1.0);
    f *= 0.2 + f2 * f2 * 0.8;
    let f1 = f1 * f2;
    p.ticks_since_last_swing = 0;
    if !(f > 0.0 || f1 > 0.0) {
        return;
    }
    let strong = f2 > 0.9;
    let sprinting = st.flags & SPRINTING != 0;
    let mut knock = kb_level;
    if sprinting && strong {
        knock += 1;
    }
    let on_ground = st.flags & proto::ON_GROUND != 0;
    // flag2 (a critical hit): falling, not sprinting, not on a ladder or in water.
    // DROPPED: riding.
    let blind = p.effects.is_active(items::potion::Effect::Blindness);
    let crit = strong && p.fall_distance > 0.0 && !on_ground && !ladder && !p.in_water && !blind && !sprinting;
    if crit {
        f *= 1.5;
    }
    if sprinting && strong {
        attack_sound(game, st.pos, "entity.player.attack.knockback");
    }
    f += f1;
    let attacker = Attacker { name, pos: st.pos, player: true, id: Some(pid) };
    // Fire Aspect: a mob not yet burning is set alight for a second before the hit, so the hurt
    // reads as fire; the full burn follows a landed hit.
    let mut set_alight = false;
    if fire_aspect > 0 {
        if let Some(i) = index_of(game, target) {
            if !game.mobs.mobs[i].is_burning() {
                set_alight = true;
                game.mobs.mobs[i].set_fire(1);
            }
        }
    }
    let landed = hurt_mob(game, target, &DamageSource::player(attacker), f);
    if !landed {
        attack_sound(game, st.pos, "entity.player.attack.nodamage");
        if set_alight {
            if let Some(i) = index_of(game, target) {
                game.mobs.mobs[i].fire = 0;
            }
        }
        return;
    }
    if knock > 0 {
        let yaw = st.yaw * 0.017453292;
        if let Some(i) = index_of(game, target) {
            let st = &mut game.mobs;
            st.mobs[i].knock_back(knock as f32 * 0.5, sin(yaw) as f64, -cos(yaw) as f64, &mut st.rand);
        }
    }
    if fire_aspect > 0 {
        if let Some(i) = index_of(game, target) {
            game.mobs.mobs[i].set_fire(fire_aspect * 4);
        }
    }
    if crit {
        // onCriticalHit: SPacketAnimation(target, 4) to the attacker and those who track it.
        let body = proto::Animation { id: target, animation: ANIM_CRIT }.encode();
        net::send(game, pid, proto::S_ANIMATION, &body);
        net::send_near(game, Some(pid), st.pos, 64.0, proto::S_ANIMATION, &body);
        attack_sound(game, st.pos, "entity.player.attack.crit");
    }
    // DROPPED: the sweep branch (`flag3`, needs the walked distance) and its sound, so every hit
    // that is not a crit plays the strong or weak sound.
    if !crit {
        attack_sound(game, st.pos, if strong { "entity.player.attack.strong" } else { "entity.player.attack.weak" });
    }
    // EnchantmentHelper.applyArthropodEnchantments: Bane of Arthropods slows a spider.
    let bane = items::enchant::get_enchantment_level(items::enchant::id::BANE_OF_ARTHROPODS, &held);
    if bane > 0 && attr == items::enchant::CreatureAttribute::Arthropod {
        let ticks = 20 + game.players.rand.next_int_bounded(10 * bane);
        if let Some(i) = index_of(game, target) {
            game.mobs.mobs[i].add_potion_effect(items::potion::PotionEffect::new(items::potion::Effect::Slowness, ticks, 3));
        }
    }
    // itemstack1.hitEntity: the held tool wears (not in creative).
    let creative = mode == proto::GM_CREATIVE;
    let mut worn = false;
    if let Some(p) = game.players.players.get_mut(&pid) {
        let slot = p.inv.current_item;
        let mut stack = p.inv.get_current_item();
        let cost = if stack.is_empty() { 0 } else { tools::hit_entity_damage(stack.get_item()) };
        if cost > 0 {
            stack.damage_item(cost, creative, &mut players::wear_rand());
            p.inv.main[slot] = if stack.is_empty() { items::ItemStack::EMPTY } else { stack };
            worn = true;
        }
    }
    if worn {
        players::send_slots(game, pid);
    }
    players::add_exhaustion(game, pid, 0.1);
}

#[cfg(test)]
#[path = "combat_tests.rs"]
mod tests;
