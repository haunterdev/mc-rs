//! Potion effects on a player: `EntityLivingBase.addPotionEffect`, `removeActivePotionEffect`,
//! `clearActivePotions`, `updatePotionEffects`, the hooks `onNewPotionEffect`,
//! `onChangedPotionEffect` and `onFinishedPotionEffect`, and `Potion.performEffect`. The data is
//! `items::potion`; mobs use the same rules in mobs/mod.rs.
//!
//! DROPPED across the file: `PotionType`, brewing and splash or lingering potions, tipped arrows,
//! `SPacketEntityEffect` for an entity that starts being tracked later (only changes are sent),
//! levitation (the server does not simulate a player's motion), the `Potion.isPotionApplicable`
//! check for a player (only an undead mob is immune to regeneration and poison, mobs/mod.rs),
//! the advancement triggers and the scoreboard.

use items::potion::{Added, Effect, PotionEffect};

use super::damage::DamageSource;
use super::{net, players, Game};

/// `EntityTrackerEntry`'s range for another player's effects.
const RANGE: f64 = 64.0;

/// The `SPacketEntityEffect` of `e` on entity `id`.
pub fn effect_msg(id: u32, e: &PotionEffect) -> proto::EntityEffect {
    let mut flags = 0;
    if e.ambient {
        flags |= proto::EntityEffect::AMBIENT;
    }
    if !e.show_particles {
        flags |= proto::EntityEffect::HIDE_PARTICLES;
    }
    proto::EntityEffect { id, effect: e.effect.id() as u8, amplifier: e.amplifier.clamp(0, 255) as u8, duration: e.duration, flags }
}

fn announce(game: &mut Game, id: u32, e: &PotionEffect) {
    let body = effect_msg(id, e).encode();
    net::send(game, id, proto::S_EFFECT, &body);
    if let Some(st) = game.sessions.get(&id).and_then(|s| s.state) {
        net::send_near(game, Some(id), st.pos, RANGE, proto::S_EFFECT, &body);
    }
}

fn announce_removed(game: &mut Game, id: u32, e: Effect) {
    let body = proto::RemoveEffect { id, effect: e.id() as u8 }.encode();
    net::send(game, id, proto::S_REMOVE_EFFECT, &body);
    if let Some(st) = game.sessions.get(&id).and_then(|s| s.state) {
        net::send_near(game, Some(id), st.pos, RANGE, proto::S_REMOVE_EFFECT, &body);
    }
}

/// `Potion.applyAttributesModifiersToEntity` for the effects that act on state the server keeps:
/// absorption and the health boost. The rest are read from the effect list where they matter.
fn apply_attributes(game: &mut Game, id: u32, e: &PotionEffect) {
    let Some(p) = game.players.players.get_mut(&id) else { return };
    if e.effect == Effect::Absorption {
        p.absorption += (4 * (e.amplifier + 1)) as f32;
    }
    p.max_health = players::MAX_HEALTH + p.effects.max_health_bonus() as f32;
}

/// `Potion.removeAttributesModifiersFromEntity`, for the effect already out of the list.
fn remove_attributes(game: &mut Game, id: u32, e: &PotionEffect) {
    let Some(p) = game.players.players.get_mut(&id) else { return };
    if e.effect == Effect::Absorption {
        p.absorption = (p.absorption - (4 * (e.amplifier + 1)) as f32).max(0.0);
    }
    p.max_health = players::MAX_HEALTH + p.effects.max_health_bonus() as f32;
    // PotionHealthBoost: a health above the new maximum is cut down.
    if p.health > p.max_health {
        p.health = p.max_health;
    }
}

/// `EntityLivingBase.addPotionEffect`.
pub fn add(game: &mut Game, id: u32, effect: PotionEffect) {
    let Some(p) = game.players.players.get_mut(&id) else { return };
    if p.dead {
        return;
    }
    match p.effects.add(effect) {
        Added::New(e) => {
            apply_attributes(game, id, &e);
            announce(game, id, &e);
        }
        Added::Changed(merged) => {
            // onChangedPotionEffect(effect, true): both calls take the merged effect's amplifier.
            remove_attributes(game, id, &merged);
            apply_attributes(game, id, &merged);
            announce(game, id, &merged);
        }
    }
    players::sync(game, id);
}

/// `EntityLivingBase.removeActivePotionEffect`.
pub fn remove(game: &mut Game, id: u32, e: Effect) {
    let Some(p) = game.players.players.get_mut(&id) else { return };
    if let Some(old) = p.effects.remove(e) {
        remove_attributes(game, id, &old);
        announce_removed(game, id, e);
        players::sync(game, id);
    }
}

/// `EntityLivingBase.clearActivePotions`. True when something was active.
pub fn clear(game: &mut Game, id: u32) -> bool {
    let Some(p) = game.players.players.get_mut(&id) else { return false };
    let old = p.effects.clear();
    for e in &old {
        remove_attributes(game, id, e);
        announce_removed(game, id, e.effect);
    }
    if !old.is_empty() {
        players::sync(game, id);
    }
    !old.is_empty()
}

/// The effects a joining or respawning client needs, with its own entity id.
pub fn send_all(game: &mut Game, id: u32) {
    let list: Vec<PotionEffect> = game.players.players.get(&id).map(|p| p.effects.sorted()).unwrap_or_default();
    for e in &list {
        net::send(game, id, proto::S_EFFECT, &effect_msg(id, e).encode());
    }
}

/// `EntityLivingBase.updatePotionEffects` and the `performEffect` calls of one tick.
pub fn tick(game: &mut Game, id: u32) {
    let Some(p) = game.players.players.get_mut(&id) else { return };
    if p.effects.is_empty() {
        return;
    }
    let t = p.effects.tick();
    for (e, amp) in t.perform {
        perform(game, id, e, amp);
    }
    for e in t.changed {
        // onChangedPotionEffect(effect, false): a resync every 600 ticks.
        announce(game, id, &e);
    }
    for e in t.finished {
        remove_attributes(game, id, &e);
        announce_removed(game, id, e.effect);
    }
}

/// `Potion.performEffect` for a player (never undead).
fn perform(game: &mut Game, id: u32, e: Effect, amp: i32) {
    let health = game.players.players.get(&id).map_or(0.0, |p| p.health);
    let max = game.players.players.get(&id).map_or(0.0, |p| p.max_health);
    match e {
        Effect::Regeneration => {
            if health < max {
                players::heal(game, id, 1.0);
            }
        }
        Effect::Poison => {
            if health > 1.0 {
                super::damage::attack_entity_from(game, id, &DamageSource::magic(), 1.0);
            }
        }
        Effect::Wither => {
            super::damage::attack_entity_from(game, id, &DamageSource::wither(), 1.0);
        }
        Effect::Hunger => players::add_exhaustion(game, id, 0.005 * (amp + 1) as f32),
        Effect::Saturation => {
            if let Some(p) = game.players.players.get_mut(&id) {
                p.food.add_stats(amp + 1, 1.0);
            }
            players::sync(game, id);
        }
        Effect::InstantHealth => players::heal(game, id, ((4i32 << (amp & 31)).max(0)) as f32),
        Effect::InstantDamage => {
            super::damage::attack_entity_from(game, id, &DamageSource::magic(), (6i32 << (amp & 31)) as f32);
        }
        _ => {}
    }
}

/// The save form of an effect list (`NBTTagCompound.writeCustomPotionEffects`): id, amplifier,
/// duration, ambient and show particles.
pub fn effects_json(list: &items::potion::Effects) -> serde_json::Value {
    serde_json::Value::Array(
        list.sorted()
            .iter()
            .map(|e| serde_json::json!({"id": e.effect.id(), "amp": e.amplifier, "dur": e.duration, "ambient": e.ambient, "particles": e.show_particles}))
            .collect(),
    )
}

/// Reads [`effects_json`] into `list`; an unknown id or a spent effect is skipped.
pub fn read_effects(list: &mut items::potion::Effects, v: Option<&serde_json::Value>) {
    for e in v.and_then(|v| v.as_array()).into_iter().flatten() {
        let Some(effect) = e.get("id").and_then(|v| v.as_i64()).and_then(|i| Effect::from_id(i as i32)) else { continue };
        let amp = e.get("amp").and_then(|v| v.as_i64()).unwrap_or(0).clamp(0, 255) as i32;
        let dur = e.get("dur").and_then(|v| v.as_i64()).unwrap_or(0).clamp(0, i32::MAX as i64) as i32;
        if dur <= 0 {
            continue;
        }
        let mut pe = PotionEffect::new(effect, dur, amp);
        pe.ambient = e.get("ambient").and_then(|v| v.as_bool()).unwrap_or(false);
        pe.show_particles = e.get("particles").and_then(|v| v.as_bool()).unwrap_or(true);
        list.add(pe);
    }
}
