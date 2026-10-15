//! Potion effects on a mob: `EntityLivingBase.addPotionEffect`, `isPotionApplicable`,
//! `updatePotionEffects` and `Potion.performEffect`, the same rules as ../effects.rs for a
//! player. Changes are queued in `Mob::effect_events` and sent to the trackers by `mobs::tick`.
//!
//! DROPPED: levitation, water breathing (a mob does not drown here), hunger and saturation (they
//! act on a player's food), `PotionType` and the particles a mob with effects spawns.

use items::potion::{Added, Effect, PotionEffect};

use super::{ai, Mob};
use crate::game::damage::DamageSource;

/// A change for the trackers.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum FxEvent {
    Set(PotionEffect),
    Gone(Effect),
}

impl Mob {
    /// `EntityLivingBase.getCreatureAttribute() == UNDEAD`: zombies and skeletons.
    pub fn is_undead(&self) -> bool {
        matches!(self.def.kind.name, "zombie" | "skeleton")
    }

    /// `EntityLivingBase.isPotionApplicable`
    pub fn is_potion_applicable(&self, e: Effect) -> bool {
        !(self.is_undead() && matches!(e, Effect::Regeneration | Effect::Poison))
    }

    /// `EntityLivingBase.addPotionEffect`. False when the effect does not apply.
    pub fn add_potion_effect(&mut self, effect: PotionEffect) -> bool {
        if !self.is_potion_applicable(effect.effect) {
            return false;
        }
        match self.effects.add(effect) {
            Added::New(e) => {
                self.apply_attributes(&e);
                self.effect_events.push(FxEvent::Set(e));
            }
            Added::Changed(e) => {
                self.remove_attributes(&e);
                self.apply_attributes(&e);
                self.effect_events.push(FxEvent::Set(e));
            }
        }
        true
    }

    fn apply_attributes(&mut self, e: &PotionEffect) {
        if e.effect == Effect::Absorption {
            self.absorption += (4 * (e.amplifier + 1)) as f32;
        }
    }

    fn remove_attributes(&mut self, e: &PotionEffect) {
        if e.effect == Effect::Absorption {
            self.absorption = (self.absorption - (4 * (e.amplifier + 1)) as f32).max(0.0);
        }
        let max = self.max_health();
        if self.health > max {
            self.health = max;
        }
    }

    /// `EntityLivingBase.removeActivePotionEffect`
    pub fn remove_potion_effect(&mut self, e: Effect) {
        if let Some(old) = self.effects.remove(e) {
            self.remove_attributes(&old);
            self.effect_events.push(FxEvent::Gone(e));
        }
    }

    /// `EntityLivingBase.clearActivePotions`
    pub fn clear_potions(&mut self) {
        for e in self.effects.clear() {
            self.remove_attributes(&e);
            self.effect_events.push(FxEvent::Gone(e.effect));
        }
    }

    /// `EntityLivingBase.updatePotionEffects`, from `onEntityUpdate`.
    pub fn tick_effects(&mut self, env: &mut ai::Env) {
        if self.effects.is_empty() {
            return;
        }
        let t = self.effects.tick();
        for (e, amp) in t.perform {
            self.perform_effect(e, amp, env);
        }
        for e in t.changed {
            self.effect_events.push(FxEvent::Set(e));
        }
        for e in t.finished {
            self.remove_attributes(&e);
            self.effect_events.push(FxEvent::Gone(e.effect));
        }
    }

    /// `Potion.performEffect`
    fn perform_effect(&mut self, e: Effect, amp: i32, env: &mut ai::Env) {
        let hurt = |m: &Mob, env: &mut ai::Env, src: DamageSource, amount: f32| {
            env.fx.push(ai::Effect::Hurt { id: m.id, source: src, amount });
        };
        match e {
            Effect::Regeneration => {
                if self.health < self.max_health() {
                    self.health = (self.health + 1.0).min(self.max_health());
                }
            }
            Effect::Poison => {
                if self.health > 1.0 {
                    hurt(self, env, DamageSource::magic(), 1.0);
                }
            }
            Effect::Wither => hurt(self, env, DamageSource::wither(), 1.0),
            Effect::InstantHealth | Effect::InstantDamage => {
                let heal = (e == Effect::InstantHealth) != self.is_undead();
                if heal {
                    let n = (4i32 << (amp & 31)).max(0) as f32;
                    self.health = (self.health + n).min(self.max_health());
                } else {
                    // Instant health hurts the undead and instant damage hurts the rest, both by
                    // 6 << amplifier magic damage.
                    hurt(self, env, DamageSource::magic(), (6i32 << (amp & 31)) as f32);
                }
            }
            _ => {}
        }
    }
}
