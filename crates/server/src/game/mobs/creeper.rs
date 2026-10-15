//! `EntityCreeper` (1.12.2), ported 1:1: `initEntityAI`, the attributes (the `MOBS` entry: 20 health,
//! speed 0.25), the fuse (`onUpdate`: `timeSinceIgnited` against `fuseTime` 30), `explode` (radius 3,
//! doubled when powered, `mobGriefing`), the `IGNITED` and `POWERED` flags (`MM_IGNITED`,
//! `MM_POWERED`) and the swell state (`MobMeta.swell`). The explosion is `game::explosion`; the
//! loot is `loot_tables/entities/creeper.json`; natural night spawning is the existing spawner
//! (`spawn_lists`). An addon may replace the swell goal
//! (`addons/epic_siege/creeper.rs`).
//!
//! DROPPED: `EntityAIAvoidEntity` for ocelots (no ocelots), `fall` adding to the fuse (the mob
//! tick has no fall-distance hook), lightning making it powered, `spawnLingeringCloud` (no potion effects on a mob), the
//! music disc and skull drops in `onDeath`, `attackEntityAsMob` returning true with no damage (the
//! `MOBS` entry has no attack damage, which is the same: the melee goal swings and nothing lands),
//! `writeEntityToNBT` of `Fuse`, `ExplosionRadius` (only `ignited` and `powered` are saved, through
//! the flags).

use super::ai::target::{HurtByTarget, NearestAttackableTarget};
use super::ai::{dist_sq, goals, melee, Ctx, Goal};
use super::Mob;

/// The `EntityCreeper` fields beyond the wire meta.
#[derive(Clone, Debug)]
pub struct Creeper {
    /// `lastActiveTime`
    pub last_active_time: i32,
    /// `timeSinceIgnited`
    pub time_since_ignited: i32,
    /// `fuseTime`
    pub fuse_time: i32,
    /// `explosionRadius`
    pub explosion_radius: i32,
    /// Set when `explode()` ran; the mob tick turns it into the explosion.
    pub exploding: bool,
}

impl Default for Creeper {
    fn default() -> Creeper {
        Creeper { last_active_time: 0, time_since_ignited: 0, fuse_time: 30, explosion_radius: 3, exploding: false }
    }
}

/// `EntityAICreeperSwell`
#[derive(Clone, Debug, Default)]
pub struct Swell {
    target: Option<u32>,
}

impl Swell {
    pub fn new() -> Swell {
        Swell::default()
    }
}

impl Goal for Swell {
    fn mutex_bits(&self) -> u32 {
        1
    }

    fn should_execute(&mut self, c: &mut Ctx<'_>) -> bool {
        let near = c.mob.attack_target.and_then(|id| c.player(id)).is_some_and(|t| dist_sq(c.mob.body.pos.to_array(), t.pos) < 9.0);
        c.mob.creeper_state() > 0 || near
    }

    fn start_executing(&mut self, c: &mut Ctx<'_>) {
        c.mob.navigator.clear_path();
        self.target = c.mob.attack_target;
    }

    fn reset_task(&mut self, _c: &mut Ctx<'_>) {
        self.target = None;
    }

    fn update_task(&mut self, c: &mut Ctx<'_>) {
        let Some(t) = self.target.and_then(|id| c.player(id)) else {
            c.mob.set_creeper_state(-1);
            return;
        };
        if dist_sq(c.mob.body.pos.to_array(), t.pos) > 49.0 {
            c.mob.set_creeper_state(-1);
        } else if !c.can_see(&t) {
            c.mob.set_creeper_state(-1);
        } else {
            c.mob.set_creeper_state(1);
        }
    }
}

impl Mob {
    /// `EntityCreeper.initEntityAI`
    pub(super) fn init_creeper_ai(&mut self) {
        // STATE starts at -1.
        self.meta.swell = -1;
        let swimming = goals::Swimming::new(self);
        self.tasks.add_task(1, swimming);
        self.tasks.add_task(2, Swell::new());
        // 3: EntityAIAvoidEntity (ocelots) is DROPPED.
        self.tasks.add_task(4, melee::AttackMelee::new(1.0, false));
        self.tasks.add_task(5, goals::Wander::avoid_water(0.8));
        self.tasks.add_task(6, goals::WatchClosest::new(8.0));
        self.tasks.add_task(6, goals::LookIdle::new());
        self.target_tasks.add_task(1, NearestAttackableTarget::new(true));
        self.target_tasks.add_task(2, HurtByTarget::new(false));
    }

    pub fn is_creeper(&self) -> bool {
        self.def.kind.name == "creeper"
    }

    /// `getCreeperState`
    pub fn creeper_state(&self) -> i32 {
        self.meta.swell as i32
    }

    /// `setCreeperState`
    pub fn set_creeper_state(&mut self, state: i32) {
        self.meta.swell = state as i8;
    }

    /// `hasIgnited`
    pub fn has_ignited(&self) -> bool {
        self.meta.flags & proto::MM_IGNITED != 0
    }

    /// `ignite`
    pub fn ignite(&mut self) {
        self.meta.flags |= proto::MM_IGNITED;
    }

    /// `getPowered`
    pub fn get_powered(&self) -> bool {
        self.meta.flags & proto::MM_POWERED != 0
    }

    pub fn set_powered(&mut self, powered: bool) {
        if powered {
            self.meta.flags |= proto::MM_POWERED;
        } else {
            self.meta.flags &= !proto::MM_POWERED;
        }
    }

    /// The part of `EntityCreeper.onUpdate` before `super.onUpdate`: the fuse. Reaching `fuseTime`
    /// is `explode()`, whose `dead = true` is set here and whose explosion the mob tick makes.
    pub(super) fn creeper_pre_update(&mut self, fx: &mut Vec<super::ai::Effect>) {
        if !self.is_creeper() || self.dead || self.health <= 0.0 {
            return;
        }
        self.creeper.last_active_time = self.creeper.time_since_ignited;
        if self.has_ignited() {
            self.set_creeper_state(1);
        }
        let i = self.creeper_state();
        if i > 0 && self.creeper.time_since_ignited == 0 {
            // playSound(ENTITY_CREEPER_PRIMED, 1.0F, 0.5F)
            self.sound(fx, "entity.creeper.primed", 1.0, 0.5);
        }
        self.creeper.time_since_ignited += i;
        if self.creeper.time_since_ignited < 0 {
            self.creeper.time_since_ignited = 0;
        }
        if self.creeper.time_since_ignited >= self.creeper.fuse_time {
            self.creeper.time_since_ignited = self.creeper.fuse_time;
            self.creeper.exploding = true;
            self.dead = true;
        }
    }

    /// `explode`'s radius: `explosionRadius * (powered ? 2 : 1)`.
    pub fn explosion_size(&self) -> f32 {
        self.creeper.explosion_radius as f32 * if self.get_powered() { 2.0 } else { 1.0 }
    }

    /// `EntityCreeper.getMaxFallHeight`
    pub(super) fn creeper_max_fall_height(&self) -> i32 {
        if self.attack_target.is_none() {
            3
        } else {
            3 + (self.health - 1.0) as i32
        }
    }
}
