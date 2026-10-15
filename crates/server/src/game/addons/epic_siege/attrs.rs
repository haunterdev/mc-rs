//! ESM-7, the attribute multipliers. `handlers/entities/GeneralEntityHandler.java` on
//! `EntityJoinWorldEvent` for an `EntityMob`: an `AttributeModifier` of operation 1 on
//! `MAX_HEALTH`, `MOVEMENT_SPEED`, `ATTACK_DAMAGE` and `KNOCKBACK_RESISTANCE`, each only while the
//! multiplier is not 1.0, and the health topped up to the new maximum when it was below it. The
//! join also raises `FOLLOW_RANGE` to at least the awareness radius (`handlers/MainHandler.java`),
//! which is what the ESM target goals then read as their target distance.
//!
//! The mod gives every modifier its own UUID and keeps a per-dimension `hasModifiers` flag so a mob
//! is only scaled once; a mob here carries one `mobs::Attrs`, so scaling is idempotent without either.
//!
//! DROPPED: `KNOCKBACK_RESISTANCE` (mc-rs's knockback has no resistance term to scale),
//! `heal(getMaxHealth())` as a packet (the health is set directly), the boss modifier
//! (`curBossMod`, `bossModCap`, the `<world>/ESM.dat` float), the creeper jockey, and the
//! `AttributeMap` itself: the multipliers sit on the `Mob` and its accessors read them.

use crate::game::mobs::Mob;
use super::config::Config;

/// `EntityJoinWorldEvent`: the multipliers and the raised follow range, then the health top-up.
pub fn apply(m: &mut Mob, cfg: &Config) {
    // MainHandler: the FOLLOW_RANGE attribute is raised to at least the Awareness Radius.
    m.attrs.follow_range = m.def.follow_range.max(cfg.awareness as f32);
    m.attrs.mod_health = cfg.mod_health;
    m.attrs.mod_speed = cfg.mod_speed;
    m.attrs.mod_damage = cfg.mod_damage;
    // EntityLivingBase.heal(getMaxHealth()): the mob comes back at its new maximum.
    let max = m.max_health();
    if m.health < max {
        m.health = max;
    }
}
