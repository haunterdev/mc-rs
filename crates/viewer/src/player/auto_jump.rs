//! `EntityPlayerSP.updateAutoJump`: after a move along the ground, a jump when a block one high
//! stands in the way and there is room above it.

use bevy::math::DVec3;
use worldgen::math_helper::{cos, sin};

use super::{look, Player};
use crate::collision::{collision_box, fast_inv_sqrt, floor, get_collision_boxes, Aabb, BlockAccess};

impl Player {
    /// `EntityPlayerSP.updateAutoJump`
    pub(super) fn update_auto_jump(&mut self, w: &impl BlockAccess, dx: f32, dz: f32) {
        if !self.auto_jump || !(self.auto_jump_time <= 0 && self.body.on_ground && !self.is_sneaking()) {
            return;
        }
        // MovementInput.getMoveVector
        let (mx, my) = (self.input.strafe, self.input.forward);
        if mx == 0.0 && my == 0.0 {
            return;
        }
        let vec3d = DVec3::new(self.body.pos.x, self.body.bb.min_y, self.body.pos.z);
        let vec3d1 = DVec3::new(self.body.pos.x + dx as f64, self.body.bb.min_y, self.body.pos.z + dz as f64);
        let mut vec3d2 = DVec3::new(dx as f64, 0.0, dz as f64);
        let f = self.ai_move_speed();
        let mut f1 = vec3d2.length_squared() as f32;
        if f1 <= 0.001 {
            let f2 = f * mx;
            let f3 = f * my;
            let f4 = sin(self.body.yaw * 0.017_453_292);
            let f5 = cos(self.body.yaw * 0.017_453_292);
            vec3d2 = DVec3::new((f2 * f5 - f3 * f4) as f64, vec3d2.y, (f3 * f5 + f2 * f4) as f64);
            f1 = vec3d2.length_squared() as f32;
            if f1 <= 0.001 {
                return;
            }
        }
        let f12 = fast_inv_sqrt(f1 as f64) as f32;
        let vec3d12 = vec3d2 * f12 as f64;
        // Entity.getForward
        let vec3d13 = look(self.body.pitch, self.body.yaw).as_dvec3();
        let f13 = (vec3d13.x * vec3d12.x + vec3d13.z * vec3d12.z) as f32;
        if f13 < -0.15 {
            return;
        }
        let (bx, mut by, bz) = (floor(self.body.pos.x), floor(self.body.bb.max_y), floor(self.body.pos.z));
        if collision_box(w.state(bx, by, bz)).is_some() {
            return;
        }
        by += 1;
        if collision_box(w.state(bx, by, bz)).is_some() {
            return;
        }
        let mut f7 = 1.2f32;
        let boost = self.effects.jump_boost_level();
        if boost > 0.0 {
            f7 += boost * 0.75;
        }
        let f8 = (f * 7.0).max(1.0 / f12);
        let mut vec3d4 = vec3d1 + vec3d12 * f8 as f64;
        let f9 = self.body.width;
        let f10 = self.body.height;
        let axisalignedbb = Aabb::of(vec3d, vec3d4 + DVec3::new(0.0, f10 as f64, 0.0)).grow(f9 as f64, 0.0, f9 as f64);
        let lift = DVec3::new(0.0, 0.509_999_990_463_256_8, 0.0);
        let lvt = vec3d + lift;
        vec3d4 += lift;
        let vec3d5 = vec3d12.cross(DVec3::Y);
        let vec3d6 = vec3d5 * (f9 * 0.5) as f64;
        let (vec3d7, vec3d8) = (lvt - vec3d6, vec3d4 - vec3d6);
        let (vec3d9, vec3d10) = (lvt + vec3d6, vec3d4 + vec3d6);
        let list = get_collision_boxes(w, &axisalignedbb);
        // Float.MIN_VALUE, the smallest positive float.
        let none = f32::from_bits(1);
        let mut f11 = none;
        'boxes: for b in &list {
            if b.intersects_span(vec3d7, vec3d8) || b.intersects_span(vec3d9, vec3d10) {
                f11 = b.max_y as f32;
                let c = b.center();
                let (px, py, pz) = (floor(c.x), floor(c.y), floor(c.z));
                let mut i = 1;
                loop {
                    if i as f32 >= f7 {
                        break 'boxes;
                    }
                    let y2 = py + i;
                    if let Some(bb1) = collision_box(w.state(px, y2, pz)) {
                        f11 = bb1.max_y as f32 + y2 as f32;
                        if f11 as f64 - self.body.bb.min_y > f7 as f64 {
                            return;
                        }
                    }
                    if i > 1 {
                        by += 1;
                        if collision_box(w.state(bx, by, bz)).is_some() {
                            return;
                        }
                    }
                    i += 1;
                }
            }
        }
        if f11 != none {
            let f14 = (f11 as f64 - self.body.bb.min_y) as f32;
            if f14 > 0.5 && f14 <= f7 {
                self.auto_jump_time = 1;
            }
        }
    }
}
