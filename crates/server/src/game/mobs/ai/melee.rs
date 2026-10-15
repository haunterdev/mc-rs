//! `EntityAIAttackMelee` and its `EntityAIZombieAttack` subclass.
//!
//! The attack itself is `EntityMob.attackEntityAsMob`, which needs the game: the goal pushes an
//! [`Effect::Attack`] and `mobs::apply_effects` lands it through `combat::attack_entity_as_mob`.
//! DROPPED: `swingArm` (no swing animation packet), `isWithinHomeDistanceFromPosition`, and a
//! target that is not a player.

use super::{dist_sq, Ctx, Effect, Goal, PlayerView, PLAYER_WIDTH};
use crate::game::mobs::path::point::Path;

/// `EntityAIAttackMelee`
#[derive(Clone, Debug)]
pub struct AttackMelee {
    pub attack_tick: i32,
    speed_towards_target: f64,
    long_memory: bool,
    path: Option<Path>,
    delay_counter: i32,
    target_x: f64,
    target_y: f64,
    target_z: f64,
}

impl AttackMelee {
    pub fn new(speed: f64, use_long_memory: bool) -> AttackMelee {
        AttackMelee {
            attack_tick: 0,
            speed_towards_target: speed,
            long_memory: use_long_memory,
            path: None,
            delay_counter: 0,
            target_x: 0.0,
            target_y: 0.0,
            target_z: 0.0,
        }
    }

    /// `getAttackTarget` as a live player.
    fn target(c: &Ctx<'_>) -> Option<PlayerView> {
        c.mob.attack_target.and_then(|id| c.player(id))
    }

    /// `getAttackReachSqr`, in `float` arithmetic like Java.
    fn get_attack_reach_sqr(c: &Ctx<'_>, _target: &PlayerView) -> f64 {
        let w = c.mob.body.width;
        (w * 2.0 * w * 2.0 + PLAYER_WIDTH) as f64
    }

    /// `getDistanceSq(target.posX, target.minY, target.posZ)` from the attacker.
    fn dist_to(c: &Ctx<'_>, t: &PlayerView) -> f64 {
        let m = c.mob.body.pos;
        dist_sq([m.x, m.y, m.z], t.pos)
    }

    /// `checkAndPerformAttack`
    fn check_and_perform_attack(&mut self, c: &mut Ctx<'_>, enemy: &PlayerView, dist_to_enemy_sqr: f64) {
        let d0 = Self::get_attack_reach_sqr(c, enemy);
        if dist_to_enemy_sqr <= d0 && self.attack_tick <= 0 {
            self.attack_tick = 20;
            // swingArm is DROPPED.
            c.fx.push(Effect::Attack { mob: c.mob.id, target: enemy.id });
        }
    }
}

impl Goal for AttackMelee {
    fn mutex_bits(&self) -> u32 {
        3
    }

    fn should_execute(&mut self, c: &mut Ctx<'_>) -> bool {
        if c.mob.attack_target.is_none() {
            return false;
        }
        let Some(t) = Self::target(c) else { return false };
        if t.dead {
            return false;
        }
        self.path = c.mob.get_path_to_entity_living(c.w, t.pos);
        if self.path.is_some() {
            true
        } else {
            Self::get_attack_reach_sqr(c, &t) >= Self::dist_to(c, &t)
        }
    }

    fn should_continue_executing(&mut self, c: &mut Ctx<'_>) -> bool {
        if c.mob.attack_target.is_none() {
            return false;
        }
        let Some(t) = Self::target(c) else { return false };
        if t.dead {
            return false;
        }
        if !self.long_memory {
            return !c.mob.navigator.no_path();
        }
        !t.spectator && !t.disable_damage
    }

    fn start_executing(&mut self, c: &mut Ctx<'_>) {
        let path = self.path.clone();
        c.mob.set_path(c.w, path, self.speed_towards_target);
        self.delay_counter = 0;
    }

    fn reset_task(&mut self, c: &mut Ctx<'_>) {
        if let Some(t) = Self::target(c) {
            if t.spectator || t.disable_damage {
                c.mob.attack_target = None;
            }
        }
        c.mob.navigator.clear_path();
    }

    fn update_task(&mut self, c: &mut Ctx<'_>) {
        let Some(t) = Self::target(c) else { return };
        c.mob.look_helper.set_look_position_with_entity(t.eye_pos(), 30.0, 30.0);
        let d0 = Self::dist_to(c, &t);
        self.delay_counter -= 1;
        if (self.long_memory || c.can_see(&t))
            && self.delay_counter <= 0
            && ((self.target_x == 0.0 && self.target_y == 0.0 && self.target_z == 0.0)
                || dist_sq(t.pos, [self.target_x, self.target_y, self.target_z]) >= 1.0
                || c.rand.next_float() < 0.05)
        {
            self.target_x = t.pos[0];
            self.target_y = t.pos[1];
            self.target_z = t.pos[2];
            self.delay_counter = 4 + c.rand.next_int_bounded(7);
            if d0 > 1024.0 {
                self.delay_counter += 10;
            } else if d0 > 256.0 {
                self.delay_counter += 5;
            }
            if !c.mob.try_move_to_entity_living(c.w, t.pos, self.speed_towards_target) {
                self.delay_counter += 15;
            }
        }
        self.attack_tick = (self.attack_tick - 1).max(0);
        self.check_and_perform_attack(c, &t, d0);
    }
}

/// `EntityAIZombieAttack`: the arms-raised flag around the melee goal.
#[derive(Clone, Debug)]
pub struct ZombieAttack {
    melee: AttackMelee,
    raise_arm_ticks: i32,
}

impl ZombieAttack {
    pub fn new(speed: f64, long_memory: bool) -> ZombieAttack {
        ZombieAttack { melee: AttackMelee::new(speed, long_memory), raise_arm_ticks: 0 }
    }
}

impl Goal for ZombieAttack {
    fn mutex_bits(&self) -> u32 {
        self.melee.mutex_bits()
    }

    fn should_execute(&mut self, c: &mut Ctx<'_>) -> bool {
        self.melee.should_execute(c)
    }

    fn should_continue_executing(&mut self, c: &mut Ctx<'_>) -> bool {
        self.melee.should_continue_executing(c)
    }

    fn start_executing(&mut self, c: &mut Ctx<'_>) {
        self.melee.start_executing(c);
        self.raise_arm_ticks = 0;
    }

    fn reset_task(&mut self, c: &mut Ctx<'_>) {
        self.melee.reset_task(c);
        c.mob.arms_raised = false;
    }

    fn update_task(&mut self, c: &mut Ctx<'_>) {
        self.melee.update_task(c);
        self.raise_arm_ticks += 1;
        c.mob.arms_raised = self.raise_arm_ticks >= 5 && self.melee.attack_tick < 10;
    }

    fn reset_on_remove(&mut self, mob: &mut crate::game::mobs::Mob) {
        mob.arms_raised = false;
    }
}

/// `AbstractSkeleton.aiAttackOnCollide`: `EntityAIAttackMelee` that sets `isSwingingArms` on
/// start and clears it on reset. It is one of the two goals `setCombatTask` swaps.
#[derive(Clone, Debug)]
pub struct SkeletonMelee {
    melee: AttackMelee,
}

impl SkeletonMelee {
    pub fn new(speed: f64, long_memory: bool) -> SkeletonMelee {
        SkeletonMelee { melee: AttackMelee::new(speed, long_memory) }
    }
}

impl Goal for SkeletonMelee {
    fn mutex_bits(&self) -> u32 {
        self.melee.mutex_bits()
    }

    fn should_execute(&mut self, c: &mut Ctx<'_>) -> bool {
        self.melee.should_execute(c)
    }

    fn should_continue_executing(&mut self, c: &mut Ctx<'_>) -> bool {
        self.melee.should_continue_executing(c)
    }

    fn start_executing(&mut self, c: &mut Ctx<'_>) {
        self.melee.start_executing(c);
        c.mob.swinging_arms = true;
    }

    fn reset_task(&mut self, c: &mut Ctx<'_>) {
        self.melee.reset_task(c);
        c.mob.swinging_arms = false;
    }

    fn update_task(&mut self, c: &mut Ctx<'_>) {
        self.melee.update_task(c);
    }

    fn reset_on_remove(&mut self, mob: &mut crate::game::mobs::Mob) {
        mob.swinging_arms = false;
    }

    fn is_combat_task(&self) -> bool {
        true
    }
}
