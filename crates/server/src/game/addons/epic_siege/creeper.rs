//! The Epic Siege creeper (ESM 13.169, `ai/ESM_EntityAICreeperSwell.java`,
//! `handlers/entities/CreeperHandler.java`), ported 1:1 with the defaults of section 4 of
//! Epic Siege's defaults: `Walking Fuse` true, `Breaching` true, `Powered` 0.1.
//!
//! The swell goal replaces `EntityAICreeperSwell`. A creeper alive more than 60 ticks with a target
//! within 64 blocks breaches: it ignores range and sight, and the first update after that locks the
//! fuse (`detLocked`), which then never cancels. With `Walking Fuse` the goal takes no mutex bit, so
//! the attack goal keeps walking the creeper at its target while it fuses. The powered roll runs
//! once when the creeper joins the world.
//!
//! DROPPED: `Napalm` (`ExplosionEvent.Start` setting `causesFire`), `Cena` (the renamed creeper, the
//! sounds and the tripled blast), `Jockey` (creeper passengers), the `isRiding` and tamed tests in
//! `canBreachEntity` (no mob rides here and a creeper cannot be tamed), the per-creeper config
//! lookups (`CfgProps.get(entity)`, so the defaults apply to every creeper), and the "already
//! modified" capability (a saved creeper keeps its powered flag through the flags instead).

use worldgen::java_random::JavaRandom;

use crate::game::mobs::ai::{dist_sq, Ctx, Goal};
use crate::game::mobs::creeper::Swell;
use crate::game::mobs::Mob;

/// `CfgProps.CR_POWERED`, the default.
pub const POWERED_CHANCE: f64 = 0.1;
/// `creeper.ticksExisted > 60` in `canBreachEntity`.
pub const BREACH_AGE: i32 = 60;
/// `creeper.getDistance(target) < 64.0f` in `canBreachEntity`.
pub const BREACH_RANGE: f32 = 64.0;
/// How long a creeper must have gone without a path before it counts as having none. The melee
/// goal finds a path only every 4 to 20 ticks and not at all before it first runs, so a bare
/// `no_path()` on one tick says nothing about a wall (it lit the fuse from across the field
/// the moment a target was taken, and the creeper then blew on arrival with no warning).
pub const NO_PATH_TICKS: i32 = 40;

/// `ESM_EntityAICreeperSwell`
#[derive(Clone, Debug)]
pub struct EsmCreeperSwell {
    attack_target: Option<u32>,
    det_locked: bool,
    blast_size: i32,
    /// `CfgProps.CR_BREACH`
    breaching: bool,
    /// `ticksExisted` when the creeper was first seen with no path, until a path is seen again.
    no_path_since: Option<i32>,
}

impl EsmCreeperSwell {
    pub fn new(breaching: bool) -> EsmCreeperSwell {
        EsmCreeperSwell { attack_target: None, det_locked: false, blast_size: -1, breaching, no_path_since: None }
    }

    /// `canBreachEntity`
    fn can_breach_entity(&mut self, c: &Ctx<'_>, target: Option<u32>) -> bool {
        // `!creeper.hasPath()`, which the original lacks: a creeper that is walking at its target
        // has a path and does not breach. Only one without a path for NO_PATH_TICKS does.
        if c.mob.navigator.no_path() {
            self.no_path_since.get_or_insert(c.mob.ticks_existed);
        } else {
            self.no_path_since = None;
        }
        if !self.breaching {
            return false;
        }
        let Some(t) = target.and_then(|id| c.player(id)) else { return false };
        let m = c.mob.body.pos;
        // Entity.getDistance(Entity): the square root in float.
        let d = (((m.x - t.pos[0]).powi(2) + (m.y - t.pos[1]).powi(2) + (m.z - t.pos[2]).powi(2)) as f32).sqrt();
        let pathless = self.no_path_since.is_some_and(|t| c.mob.ticks_existed - t >= NO_PATH_TICKS);
        c.mob.ticks_existed > BREACH_AGE && pathless && d < BREACH_RANGE
    }
}

impl Goal for EsmCreeperSwell {
    /// `Walking Fuse` true: no `setMutexBits(1)`.
    fn mutex_bits(&self) -> u32 {
        0
    }

    fn should_execute(&mut self, c: &mut Ctx<'_>) -> bool {
        let target = c.mob.attack_target;
        if self.blast_size < 0 {
            self.blast_size = c.mob.creeper.explosion_radius;
        }
        let r = (self.blast_size * self.blast_size) as f64;
        let near = target.and_then(|id| c.player(id)).is_some_and(|t| dist_sq(c.mob.body.pos.to_array(), t.pos) < r);
        c.mob.creeper_state() > 0 || self.can_breach_entity(c, target) || near
    }

    fn start_executing(&mut self, c: &mut Ctx<'_>) {
        self.attack_target = c.mob.attack_target;
    }

    fn reset_task(&mut self, _c: &mut Ctx<'_>) {
        self.attack_target = None;
    }

    fn update_task(&mut self, c: &mut Ctx<'_>) {
        if self.det_locked {
            c.mob.set_creeper_state(1);
            return;
        }
        let final_blast = self.blast_size * if c.mob.get_powered() { 2 } else { 1 };
        let breaching = self.can_breach_entity(c, self.attack_target);
        let target = self.attack_target.and_then(|id| c.player(id));
        match target {
            None => c.mob.set_creeper_state(-1),
            Some(t) if dist_sq(c.mob.body.pos.to_array(), t.pos) > (final_blast * final_blast) as f64 && !breaching => c.mob.set_creeper_state(-1),
            Some(t) if !c.can_see(&t) && !breaching => c.mob.set_creeper_state(-1),
            Some(_) => {
                self.det_locked = true;
                c.mob.set_creeper_state(1);
            }
        }
    }
}

/// `ModifierCreeperSwell`: the vanilla swell goal in place becomes the Epic Siege one, at the same
/// priority.
pub fn replace_swell(m: &mut Mob, breaching: bool) {
    for e in m.tasks.entries.iter_mut() {
        if e.action.as_any().is::<Swell>() {
            e.action = Box::new(EsmCreeperSwell::new(breaching));
        }
    }
}

/// `CreeperHandler.onSpawn`: once, when a creeper joins the world, `chance` of the time it is
/// powered.
pub fn on_spawn(m: &mut Mob, rand: &mut JavaRandom, chance: f64) {
    if m.is_creeper() && rand.next_double() < chance {
        m.set_powered(true);
    }
}
