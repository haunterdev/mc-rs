//! `EntityMoveHelper`, `EntityJumpHelper` and `EntityLookHelper`: the controls an `EntityLiving`
//! drives from its AI. They write `moveForward`, `moveStrafing`, `isJumping`, `landMovementFactor`
//! and the angles of a [`NavEntity`]; [`super::super::Mob`] copies them back and
//! `EntityLivingBase.onLivingUpdate` and `travel` act on them.

use sim::collision::{floor, BlockAccess};
use worldgen::math_helper::{cos, sin};

use super::math::{atan2, sqrt_f, wrap_degrees};
use super::node::{NavEntity, NodeProcessor, PathNodeType};
use super::walk::WalkNodeProcessor;

impl NavEntity {
    /// `EntityLiving.setAIMoveSpeed`: also sets `moveForward` to the same value.
    pub fn set_ai_move_speed(&mut self, v: f32) {
        self.ai_move_speed = v;
        self.move_forward = v;
    }
}

/// `EntityMoveHelper.Action`
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MoveAction {
    Wait,
    MoveTo,
    Strafe,
    Jumping,
}

/// `EntityMoveHelper`
#[derive(Clone, Debug)]
pub struct EntityMoveHelper {
    pos_x: f64,
    pos_y: f64,
    pos_z: f64,
    speed: f64,
    move_forward: f32,
    move_strafe: f32,
    pub action: MoveAction,
}

impl Default for EntityMoveHelper {
    fn default() -> EntityMoveHelper {
        EntityMoveHelper { pos_x: 0.0, pos_y: 0.0, pos_z: 0.0, speed: 0.0, move_forward: 0.0, move_strafe: 0.0, action: MoveAction::Wait }
    }
}

impl EntityMoveHelper {
    pub fn is_updating(&self) -> bool {
        self.action == MoveAction::MoveTo
    }

    pub fn get_speed(&self) -> f64 {
        self.speed
    }

    /// `EntityMoveHelper.setMoveTo`
    pub fn set_move_to(&mut self, x: f64, y: f64, z: f64, speed: f64) {
        self.pos_x = x;
        self.pos_y = y;
        self.pos_z = z;
        self.speed = speed;
        self.action = MoveAction::MoveTo;
    }

    /// `EntityMoveHelper.strafe`
    pub fn strafe(&mut self, forward: f32, strafe: f32) {
        self.action = MoveAction::Strafe;
        self.move_forward = forward;
        self.move_strafe = strafe;
        self.speed = 0.25;
    }

    pub fn get_x(&self) -> f64 {
        self.pos_x
    }

    pub fn get_y(&self) -> f64 {
        self.pos_y
    }

    pub fn get_z(&self) -> f64 {
        self.pos_z
    }

    /// `EntityMoveHelper.onUpdateMoveHelper`. `nodes` is `getNavigator().getNodeProcessor()`.
    pub fn on_update_move_helper(&mut self, w: &impl BlockAccess, e: &mut NavEntity, jump: &mut EntityJumpHelper, nodes: &WalkNodeProcessor) {
        match self.action {
            MoveAction::Strafe => {
                let f = e.movement_speed;
                let mut f1 = self.speed as f32 * f;
                let mut f2 = self.move_forward;
                let mut f3 = self.move_strafe;
                let mut f4 = sqrt_f(f2 * f2 + f3 * f3);
                if f4 < 1.0 {
                    f4 = 1.0;
                }
                f4 = f1 / f4;
                f2 *= f4;
                f3 *= f4;
                let f5 = sin(e.yaw * 0.017_453_292);
                let f6 = cos(e.yaw * 0.017_453_292);
                let f7 = f2 * f6 - f3 * f5;
                let f8 = f3 * f6 + f2 * f5;
                if nodes.get_path_node_type(w, floor(e.pos.x + f7 as f64), floor(e.pos.y), floor(e.pos.z + f8 as f64)) != PathNodeType::Walkable {
                    self.move_forward = 1.0;
                    self.move_strafe = 0.0;
                    f1 = f;
                }
                e.set_ai_move_speed(f1);
                e.move_forward = self.move_forward;
                e.move_strafing = self.move_strafe;
                self.action = MoveAction::Wait;
            }
            MoveAction::MoveTo => {
                self.action = MoveAction::Wait;
                let d0 = self.pos_x - e.pos.x;
                let d1 = self.pos_z - e.pos.z;
                let d2 = self.pos_y - e.pos.y;
                let d3 = d0 * d0 + d2 * d2 + d1 * d1;
                if d3 < 2.500_000_277_905_201e-7 {
                    e.move_forward = 0.0;
                    return;
                }
                let f9 = (atan2(d1, d0) * 57.295_776_367_187_5) as f32 - 90.0;
                e.yaw = self.limit_angle(e.yaw, f9, 90.0);
                e.set_ai_move_speed((self.speed * e.movement_speed as f64) as f32);
                if d2 > e.step_height as f64 && d0 * d0 + d1 * d1 < e.width.max(1.0) as f64 {
                    jump.set_jumping();
                    self.action = MoveAction::Jumping;
                }
            }
            MoveAction::Jumping => {
                e.set_ai_move_speed((self.speed * e.movement_speed as f64) as f32);
                if e.on_ground {
                    self.action = MoveAction::Wait;
                }
            }
            MoveAction::Wait => e.move_forward = 0.0,
        }
    }

    /// `EntityMoveHelper.limitAngle`
    fn limit_angle(&self, source: f32, target: f32, max_change: f32) -> f32 {
        let mut f = wrap_degrees(target - source);
        if f > max_change {
            f = max_change;
        }
        if f < -max_change {
            f = -max_change;
        }
        let mut f1 = source + f;
        if f1 < 0.0 {
            f1 += 360.0;
        } else if f1 > 360.0 {
            f1 -= 360.0;
        }
        f1
    }
}

/// `EntityJumpHelper`
#[derive(Clone, Debug, Default)]
pub struct EntityJumpHelper {
    is_jumping: bool,
}

impl EntityJumpHelper {
    pub fn set_jumping(&mut self) {
        self.is_jumping = true;
    }

    /// `EntityJumpHelper.doJump`
    pub fn do_jump(&mut self, e: &mut NavEntity) {
        e.is_jumping = self.is_jumping;
        self.is_jumping = false;
    }
}

/// `EntityLookHelper`
#[derive(Clone, Debug, Default)]
pub struct EntityLookHelper {
    delta_look_yaw: f32,
    delta_look_pitch: f32,
    is_looking: bool,
    pos_x: f64,
    pos_y: f64,
    pos_z: f64,
}

impl EntityLookHelper {
    /// `EntityLookHelper.setLookPositionWithEntity`, for a target whose look point is `at`
    /// (its eye for a living entity, the box centre otherwise).
    pub fn set_look_position_with_entity(&mut self, at: [f64; 3], delta_yaw: f32, delta_pitch: f32) {
        self.set_look_position(at[0], at[1], at[2], delta_yaw, delta_pitch);
    }

    /// `EntityLookHelper.setLookPosition`
    pub fn set_look_position(&mut self, x: f64, y: f64, z: f64, delta_yaw: f32, delta_pitch: f32) {
        self.pos_x = x;
        self.pos_y = y;
        self.pos_z = z;
        self.delta_look_yaw = delta_yaw;
        self.delta_look_pitch = delta_pitch;
        self.is_looking = true;
    }

    pub fn get_is_looking(&self) -> bool {
        self.is_looking
    }

    /// `EntityLookHelper.onUpdateLook`. `has_path` is `!getNavigator().noPath()`.
    pub fn on_update_look(&mut self, e: &mut NavEntity, has_path: bool) {
        e.pitch = 0.0;
        if self.is_looking {
            self.is_looking = false;
            let d0 = self.pos_x - e.pos.x;
            let d1 = self.pos_y - (e.pos.y + e.eye_height as f64);
            let d2 = self.pos_z - e.pos.z;
            let d3 = sqrt_f((d0 * d0 + d2 * d2) as f32) as f64;
            let f = (atan2(d2, d0) * 57.295_776_367_187_5) as f32 - 90.0;
            let f1 = (-(atan2(d1, d3) * 57.295_776_367_187_5)) as f32;
            e.pitch = Self::update_rotation(e.pitch, f1, self.delta_look_pitch);
            e.head_yaw = Self::update_rotation(e.head_yaw, f, self.delta_look_yaw);
        } else {
            e.head_yaw = Self::update_rotation(e.head_yaw, e.render_yaw_offset, 10.0);
        }
        let f2 = wrap_degrees(e.head_yaw - e.render_yaw_offset);
        if has_path {
            if f2 < -75.0 {
                e.head_yaw = e.render_yaw_offset - 75.0;
            }
            if f2 > 75.0 {
                e.head_yaw = e.render_yaw_offset + 75.0;
            }
        }
    }

    /// `EntityLookHelper.updateRotation`
    fn update_rotation(from: f32, to: f32, max: f32) -> f32 {
        let mut f = wrap_degrees(to - from);
        if f > max {
            f = max;
        }
        if f < -max {
            f = -max;
        }
        from + f
    }
}
