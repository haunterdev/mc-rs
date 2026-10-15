//! `EntityAIAttackRangedBow`: the skeleton's bow goal. It draws the bow (the main hand's active
//! hand), strafes while the target is in range and in sight, and at 20 ticks of draw shoots
//! through `attackEntityWithRangedAttack` (`skeleton::attack_entity_with_ranged_attack`).
//!
//! DROPPED: a target that is not a player (the attack target is a session id).

use super::{dist_sq, Ctx, Goal, PlayerView};
use crate::game::mobs::path::math::{atan2, wrap_degrees};
use crate::game::mobs::skeleton;

/// `EntityAIAttackRangedBow`
#[derive(Clone, Debug)]
pub struct AttackRangedBow {
    move_speed_amp: f64,
    attack_cooldown: i32,
    max_attack_distance: f32,
    attack_time: i32,
    see_time: i32,
    strafing_clockwise: bool,
    strafing_backwards: bool,
    strafing_time: i32,
}

impl AttackRangedBow {
    pub fn new(move_speed_amp: f64, attack_cooldown: i32, max_attack_distance: f32) -> AttackRangedBow {
        AttackRangedBow {
            move_speed_amp,
            attack_cooldown,
            max_attack_distance: max_attack_distance * max_attack_distance,
            attack_time: -1,
            see_time: 0,
            strafing_clockwise: false,
            strafing_backwards: false,
            strafing_time: -1,
        }
    }

    /// `setAttackCooldown`
    pub fn set_attack_cooldown(&mut self, cooldown: i32) {
        self.attack_cooldown = cooldown;
    }

    fn target(c: &Ctx<'_>) -> Option<PlayerView> {
        c.mob.attack_target.and_then(|id| c.player(id))
    }

    fn is_bow_in_mainhand(c: &Ctx<'_>) -> bool {
        c.mob.main_hand.get_item() == items::ids::item::BOW
    }
}

/// `EntityLiving.updateRotation`
fn update_rotation(angle: f32, target: f32, max: f32) -> f32 {
    let f = wrap_degrees(target - angle).clamp(-max, max);
    angle + f
}

/// `EntityLiving.faceEntity(target, maxYaw, maxPitch)` for a living target.
fn face_entity(c: &mut Ctx<'_>, t: &PlayerView, max_yaw: f32, max_pitch: f32) {
    let eye = c.mob.eye_height() as f64;
    let b = &mut c.mob.body;
    let d0 = t.pos[0] - b.pos.x;
    let d2 = t.pos[2] - b.pos.z;
    let d1 = t.pos[1] + t.eye_height as f64 - (b.pos.y + eye);
    let d3 = (d0 * d0 + d2 * d2).sqrt();
    let f = (atan2(d2, d0) * 57.295_779_513_082_32) as f32 - 90.0;
    let f1 = (-(atan2(d1, d3) * 57.295_779_513_082_32)) as f32;
    b.pitch = update_rotation(b.pitch, f1, max_pitch);
    b.yaw = update_rotation(b.yaw, f, max_yaw);
}

impl Goal for AttackRangedBow {
    fn mutex_bits(&self) -> u32 {
        3
    }

    fn should_execute(&mut self, c: &mut Ctx<'_>) -> bool {
        c.mob.attack_target.is_some() && Self::is_bow_in_mainhand(c)
    }

    fn should_continue_executing(&mut self, c: &mut Ctx<'_>) -> bool {
        (self.should_execute(c) || !c.mob.navigator.no_path()) && Self::is_bow_in_mainhand(c)
    }

    fn start_executing(&mut self, c: &mut Ctx<'_>) {
        c.mob.swinging_arms = true;
    }

    fn reset_task(&mut self, c: &mut Ctx<'_>) {
        c.mob.swinging_arms = false;
        self.see_time = 0;
        self.attack_time = -1;
        c.mob.reset_active_hand();
    }

    fn reset_on_remove(&mut self, mob: &mut crate::game::mobs::Mob) {
        mob.swinging_arms = false;
        mob.reset_active_hand();
    }

    fn is_combat_task(&self) -> bool {
        true
    }

    fn update_task(&mut self, c: &mut Ctx<'_>) {
        let Some(t) = Self::target(c) else { return };
        let m = c.mob.body.pos;
        let d0 = dist_sq([m.x, m.y, m.z], t.pos);
        let flag = c.can_see(&t);
        let flag1 = self.see_time > 0;
        if flag != flag1 {
            self.see_time = 0;
        }
        if flag {
            self.see_time += 1;
        } else {
            self.see_time -= 1;
        }
        if d0 <= self.max_attack_distance as f64 && self.see_time >= 20 {
            c.mob.navigator.clear_path();
            self.strafing_time += 1;
        } else {
            c.mob.try_move_to_entity_living(c.w, t.pos, self.move_speed_amp);
            self.strafing_time = -1;
        }
        if self.strafing_time >= 20 {
            if (c.rand.next_float() as f64) < 0.3 {
                self.strafing_clockwise = !self.strafing_clockwise;
            }
            if (c.rand.next_float() as f64) < 0.3 {
                self.strafing_backwards = !self.strafing_backwards;
            }
            self.strafing_time = 0;
        }
        if self.strafing_time > -1 {
            if d0 > (self.max_attack_distance * 0.75) as f64 {
                self.strafing_backwards = false;
            } else if d0 < (self.max_attack_distance * 0.25) as f64 {
                self.strafing_backwards = true;
            }
            c.mob.move_helper.strafe(if self.strafing_backwards { -0.5 } else { 0.5 }, if self.strafing_clockwise { 0.5 } else { -0.5 });
            face_entity(c, &t, 30.0, 30.0);
        } else {
            c.mob.look_helper.set_look_position_with_entity(t.eye_pos(), 30.0, 30.0);
        }
        if c.mob.hand_active {
            if !flag && self.see_time < -60 {
                c.mob.reset_active_hand();
            } else if flag {
                let i = c.mob.get_item_in_use_max_count();
                if i >= 20 {
                    c.mob.reset_active_hand();
                    skeleton::attack_entity_with_ranged_attack(c, &t, get_arrow_velocity(i));
                    self.attack_time = self.attack_cooldown;
                }
            }
        } else {
            self.attack_time -= 1;
            if self.attack_time <= 0 && self.see_time >= -60 {
                c.mob.set_active_hand();
            }
        }
    }
}

/// `ItemBow.getArrowVelocity`
fn get_arrow_velocity(charge: i32) -> f32 {
    let mut f = charge as f32 / 20.0;
    f = (f * f + f * 2.0) / 3.0;
    if f > 1.0 {
        f = 1.0;
    }
    f
}
