//! ESM-5, the melee. `ai/ESM_EntityAIAttackMelee.java` and its `ESM_EntityAIZombieAttack`
//! subclass, ported line by line.
//!
//! The differences from `EntityAIAttackMelee` are the long memory the modifiers ask for (the goal
//! is kept while the target lives, whatever the path does), the pathing penalty: a re-path adds
//! `ceil(dist / 8) * 10` ticks to the delay while the path does not end on the target, a failed
//! re-path adds 15 more, and the delay is capped at 60. The attack roll is `10 + rand(10)` rather
//! than the vanilla 20, and the attack is taken within the reach whenever the cooldown is up.
//! `getAtkRangeSq` is the vanilla formula.
//!
//! DROPPED: `swingArm` (no swing packet), the `EntityAnimal` branch of `func_75246_d`
//! (`attackEntityFrom(causeMobDamage, 1.0)`: no animal has this goal), and
//! `isWithinHomeDistanceFromPosition` (no mob has a home), so the `EntityCreature` branch of
//! `should_continue_executing` is always true.
//!
//! DEVIATION: the mod reads the vanilla goal's `speedTowardsTarget` reflectively; that field is
//! private to `ai::melee`, and every ported kind builds its melee goal with 1.0, so the
//! replacement carries 1.0. `canStrafe` is rolled from the shared generator on the first update
//! rather than in the constructor, which has no generator to draw from.

use crate::game::mobs::ai::{dist_sq, Ctx, Effect, Goal, PlayerView, PLAYER_WIDTH};
use crate::game::mobs::path::point::Path;

use super::config::Config;
use super::senses;

/// `ESM_EntityAIAttackMelee`
#[derive(Clone, Debug)]
pub struct EsAttackMelee {
    pub attack_tick: i32,
    speed_towards_target: f64,
    long_memory: bool,
    /// `entityPathEntity`
    path: Option<Path>,
    delay_counter: i32,
    strafe_clockwise: bool,
    can_strafe: bool,
    strafe_rolled: bool,
    /// `Strafe Evade`
    strafe: bool,
    /// `Xray Distance`, the range `should_continue_executing` keeps a target inside without a path.
    xray: f64,
}

impl EsAttackMelee {
    pub fn new(cfg: &Config, speed: f64, long_memory: bool) -> EsAttackMelee {
        EsAttackMelee {
            attack_tick: 0,
            speed_towards_target: speed,
            long_memory,
            path: None,
            delay_counter: 0,
            strafe_clockwise: true,
            can_strafe: true,
            strafe_rolled: false,
            strafe: cfg.strafe,
            xray: cfg.xray,
        }
    }

    /// `getAttackTarget` as a live player.
    fn target(c: &Ctx<'_>) -> Option<PlayerView> {
        c.mob.attack_target.and_then(|id| c.player(id))
    }

    /// `getAtkRangeSq`, in `float` arithmetic like Java.
    fn get_atk_range_sqr(c: &Ctx<'_>, _target: &PlayerView) -> f64 {
        let w = c.mob.body.width;
        (w * 2.0 * w * 2.0 + PLAYER_WIDTH) as f64
    }

    /// `getDistanceSq(target.posX, target.getEntityBoundingBox().minY, target.posZ)`.
    fn dist_sq_to(c: &Ctx<'_>, t: &PlayerView) -> f64 {
        let m = c.mob.body.pos;
        dist_sq([m.x, m.y, m.z], t.pos)
    }

    /// `ceil(dist / 8.0) * 10`, the ticks a re-path waits.
    fn pathing_penalty(dist: f32) -> i32 {
        (f64::from(dist) / 8.0).ceil() as i32 * 10
    }
}

impl Goal for EsAttackMelee {
    fn mutex_bits(&self) -> u32 {
        3
    }

    fn should_execute(&mut self, c: &mut Ctx<'_>) -> bool {
        let Some(t) = Self::target(c) else { return false };
        if t.dead {
            return false;
        }
        let dist = Self::dist_sq_to(c, &t).sqrt() as f32;
        self.delay_counter -= 1;
        if self.delay_counter > 0 || f64::from(dist) <= Self::get_atk_range_sqr(c, &t).sqrt() {
            return true;
        }
        self.path = c.mob.get_path_to_entity_living(c.w, t.pos);
        self.delay_counter = (4 + c.rand.next_int_bounded(7)).max(Self::pathing_penalty(dist));
        self.path.is_some()
    }

    fn should_continue_executing(&mut self, c: &mut Ctx<'_>) -> bool {
        let Some(t) = Self::target(c) else { return false };
        if t.dead {
            return false;
        }
        if !self.long_memory && f64::from(Self::dist_sq_to(c, &t).sqrt() as f32) > self.xray {
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
        if t.dead {
            return;
        }
        c.mob.look_helper.set_look_position_with_entity(t.eye_pos(), 30.0, 30.0);
        let dist_sq_target = Self::dist_sq_to(c, &t);
        let dist = dist_sq_target.sqrt() as f32;
        let atk_range_sq = Self::get_atk_range_sqr(c, &t);
        if !self.strafe_rolled {
            self.strafe_rolled = true;
            self.can_strafe = c.rand.next_int_bounded(10) != 0;
        }
        self.delay_counter -= 1;
        // DEVIATION: a mob in the air (a knockback) cannot path (`PathNavigateGround.canNavigate`),
        // so the re-path waits for the landing instead of failing and charging the 15 tick penalty
        // and the distance penalty, which left a zombie standing about 30 ticks after every hit.
        if (self.long_memory || senses::can_see(c, &t, self.xray))
            && self.delay_counter <= 0
            && c.mob.can_navigate(c.w)
            && (dist_sq_target >= 1.0 || c.rand.next_float() < 0.05)
        {
            self.delay_counter = 4 + c.rand.next_int_bounded(7);
            let mut pathing_penalty = 0;
            let mut on_target = false;
            match c.mob.navigator.get_path().and_then(|p| p.get_final_path_point()) {
                Some(pt) => {
                    let end = [pt.x as f64, pt.y as f64, pt.z as f64];
                    if dist_sq(t.pos, end) < 1.0 {
                        on_target = true;
                    } else {
                        pathing_penalty = Self::pathing_penalty(dist);
                    }
                }
                None => pathing_penalty = Self::pathing_penalty(dist),
            }
            self.delay_counter += pathing_penalty;
            if !on_target && !c.mob.try_move_to_entity_living(c.w, t.pos, self.speed_towards_target) {
                self.delay_counter += 15;
            }
            if self.delay_counter >= 60 {
                self.delay_counter = 60;
            }
        }
        if self.attack_tick > 0 {
            self.attack_tick -= 1;
        }
        if self.can_strafe && self.strafe && self.attack_tick > 0 && dist < 2.0 && c.can_see(&t) {
            c.mob.move_helper.strafe(0.5, if self.strafe_clockwise { 0.5 } else { -0.5 });
            c.mob.look_helper.set_look_position_with_entity(t.eye_pos(), 30.0, 30.0);
        }
        if dist_sq_target <= atk_range_sq && self.attack_tick <= 0 {
            self.strafe_clockwise = c.rand.next_boolean();
            self.attack_tick = 10 + c.rand.next_int_bounded(10);
            c.fx.push(Effect::Attack { mob: c.mob.id, target: t.id });
            self.delay_counter = 0;
        }
    }
}

/// `ESM_EntityAIZombieAttack`: the arms-raised flag around the melee goal.
#[derive(Clone, Debug)]
pub struct EsZombieAttack {
    melee: EsAttackMelee,
    raise_arm_ticks: i32,
}

impl EsZombieAttack {
    pub fn new(cfg: &Config, speed: f64, long_memory: bool) -> EsZombieAttack {
        EsZombieAttack { melee: EsAttackMelee::new(cfg, speed, long_memory), raise_arm_ticks: 0 }
    }
}

impl Goal for EsZombieAttack {
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
}
