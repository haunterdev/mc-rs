//! `Body`: the `Entity` and `EntityLivingBase` half of movement, moved from the viewer's
//! player.rs. `Entity.setPosition`, `Entity.move` with its step-up and sneak edge guard,
//! `moveRelative`, `handleWaterMovement`, `isInLava`, `isOffsetPositionInLiquid`,
//! `resetPositionToBB`, `EntityPlayerSP.pushOutOfBlocks`, and `EntityLivingBase.travel` as
//! [`travel_living`].
//!
//! Positions and motion are doubles and the angles and speeds floats, as in the game, so the
//! arithmetic rounds the same way. What the subclass decides (sneaking, flying, the movement speed
//! and the jump factor) comes in as parameters.

use glam::DVec3;
use worldgen::math_helper::{cos, sin, sqrt_f};

use crate::collision::{
    collides_with_any_block, contains_any_liquid, floor, get_collision_boxes, handle_material_acceleration, is_material_in_bb,
    is_normal_cube, slipperiness, Aabb, BlockAccess,
};
use crate::region::Mat;

/// A sound `Entity.move` or `handleWaterMovement` plays.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Sound {
    /// `playStepSound` for the block at this position.
    Step(i32, i32, i32),
    /// `getSwimSound` at this volume.
    Swim(f32),
    /// `doWaterSplashEffect`'s sound at this volume.
    Splash(f32),
}

/// The `Entity` fields movement reads and writes.
#[derive(Clone, Debug)]
pub struct Body {
    /// `posX`, `posY`, `posZ`: the feet.
    pub pos: DVec3,
    pub prev_pos: DVec3,
    pub bb: Aabb,
    pub motion: DVec3,
    /// `rotationYaw`, `rotationPitch`, in degrees.
    pub yaw: f32,
    pub pitch: f32,
    pub prev_yaw: f32,
    pub prev_pitch: f32,
    pub on_ground: bool,
    pub collided_horizontally: bool,
    pub collided_vertically: bool,
    pub width: f32,
    pub height: f32,
    pub step_height: f32,
    pub distance_walked_modified: f32,
    pub prev_distance_walked_modified: f32,
    /// `Entity.inWater`
    pub in_water: bool,
    /// `Entity.distanceWalkedOnStepModified`, `nextStepDistance`
    pub distance_walked_on_step: f32,
    pub next_step_distance: i32,
    /// Sounds the ticks asked for, played and cleared by whoever owns the body.
    pub sounds: Vec<Sound>,
}

/// What `EntityLivingBase.travel` calls back into: `Entity.move` is overridden per entity
/// (`EntityPlayerSP.move` adds auto-jump after it).
pub trait Living {
    fn body(&mut self) -> &mut Body;
    /// `move(MoverType.SELF, x, y, z)` as this entity overrides it.
    fn move_self(&mut self, w: &impl BlockAccess, x: f64, y: f64, z: f64);
}

impl Body {
    /// A 0.6 by 1.8 body standing at `feet`, with `EntityLivingBase`'s 0.6 step.
    pub fn new(feet: DVec3, yaw: f32, pitch: f32) -> Body {
        let mut b = Body {
            pos: feet,
            prev_pos: feet,
            bb: Aabb::new(0.0, 0.0, 0.0, 0.0, 0.0, 0.0),
            motion: DVec3::ZERO,
            yaw,
            pitch,
            prev_yaw: yaw,
            prev_pitch: pitch,
            on_ground: false,
            collided_horizontally: false,
            collided_vertically: false,
            width: 0.6,
            height: 1.8,
            step_height: 0.6,
            distance_walked_modified: 0.0,
            prev_distance_walked_modified: 0.0,
            in_water: false,
            distance_walked_on_step: 0.0,
            next_step_distance: 1,
            sounds: Vec::new(),
        };
        b.set_position(feet);
        b
    }

    /// `Entity.setPosition`
    pub fn set_position(&mut self, p: DVec3) {
        self.pos = p;
        let f = (self.width / 2.0) as f64;
        let f1 = self.height as f64;
        self.bb = Aabb::new(p.x - f, p.y, p.z - f, p.x + f, p.y + f1, p.z + f);
    }

    /// `Entity.isInLava`
    pub fn is_in_lava(&self, w: &impl BlockAccess) -> bool {
        is_material_in_bb(w, &self.bb.grow(-0.100_000_001_490_116_12, -0.400_000_005_960_464_5, -0.100_000_001_490_116_12), Mat::Lava)
    }

    /// `Entity.isOffsetPositionInLiquid`, which is true when the offset box is clear of blocks
    /// and liquid alike.
    pub fn is_offset_position_in_liquid(&self, w: &impl BlockAccess, x: f64, y: f64, z: f64) -> bool {
        let bb = self.bb.offset(x, y, z);
        get_collision_boxes(w, &bb).is_empty() && !contains_any_liquid(w, &bb)
    }

    /// `Entity.handleWaterMovement`, with `doWaterSplashEffect`'s sound on entering water.
    /// DROPPED: the splash particles, `fallDistance` and `extinguish`.
    pub fn handle_water_movement(&mut self, w: &impl BlockAccess) {
        let bb = self.bb.grow(0.0, -0.400_000_005_960_464_5, 0.0).shrink(0.001);
        let was = self.in_water;
        self.in_water = handle_material_acceleration(w, &bb, Mat::Water);
        if self.in_water && !was {
            let m = self.motion;
            let f1 = ((m.x * m.x * 0.20000000298023224 + m.y * m.y + m.z * m.z * 0.20000000298023224).sqrt() as f32 * 0.2).min(1.0);
            self.sounds.push(Sound::Splash(f1));
        }
    }

    /// `Entity.moveRelative`
    pub fn move_relative(&mut self, mut strafe: f32, mut up: f32, mut forward: f32, friction: f32) {
        let mut f = strafe * strafe + up * up + forward * forward;
        if f >= 1.0e-4 {
            f = sqrt_f(f);
            if f < 1.0 {
                f = 1.0;
            }
            f = friction / f;
            strafe *= f;
            up *= f;
            forward *= f;
            let f1 = sin(self.yaw * 0.017_453_292);
            let f2 = cos(self.yaw * 0.017_453_292);
            self.motion.x += (strafe * f2 - forward * f1) as f64;
            self.motion.y += up as f64;
            self.motion.z += (forward * f2 + strafe * f1) as f64;
        }
    }

    /// `Entity.resetPositionToBB`
    pub fn reset_position_to_bb(&mut self) {
        let b = self.bb;
        self.pos = DVec3::new((b.min_x + b.max_x) / 2.0, b.min_y, (b.min_z + b.max_z) / 2.0);
    }

    /// `Entity.move(MoverType.SELF, ...)`. `sneaking` is `isSneaking` (the edge guard and the
    /// walk check), `can_trigger_walking` is `canTriggerWalking` (a flying player does not).
    pub fn move_entity(&mut self, w: &impl BlockAccess, mut x: f64, mut y: f64, mut z: f64, sneaking: bool, can_trigger_walking: bool) {
        // DROPPED: noClip (spectators), the piston deltas and cobwebs.
        let (d10, d1) = (self.pos.x, self.pos.z);
        let mut d2 = x;
        let d3 = y;
        let mut d4 = z;
        if self.on_ground && sneaking {
            let down = -(self.step_height as f64);
            let edge = |v: f64| {
                if (-0.05..0.05).contains(&v) {
                    0.0
                } else if v > 0.0 {
                    v - 0.05
                } else {
                    v + 0.05
                }
            };
            while x != 0.0 && !collides_with_any_block(w, &self.bb.offset(x, down, 0.0)) {
                x = edge(x);
                d2 = x;
            }
            while z != 0.0 && !collides_with_any_block(w, &self.bb.offset(0.0, down, z)) {
                z = edge(z);
                d4 = z;
            }
            while x != 0.0 && z != 0.0 && !collides_with_any_block(w, &self.bb.offset(x, down, z)) {
                x = edge(x);
                d2 = x;
                z = edge(z);
                d4 = z;
            }
        }

        let list1 = get_collision_boxes(w, &self.bb.expand(x, y, z));
        let axisalignedbb = self.bb;
        if y != 0.0 {
            for b in &list1 {
                y = b.calculate_y_offset(&self.bb, y);
            }
            self.bb = self.bb.offset(0.0, y, 0.0);
        }
        if x != 0.0 {
            for b in &list1 {
                x = b.calculate_x_offset(&self.bb, x);
            }
            if x != 0.0 {
                self.bb = self.bb.offset(x, 0.0, 0.0);
            }
        }
        if z != 0.0 {
            for b in &list1 {
                z = b.calculate_z_offset(&self.bb, z);
            }
            if z != 0.0 {
                self.bb = self.bb.offset(0.0, 0.0, z);
            }
        }

        // The decompile reads `y != y` here and in collidedVertically; the source compares the
        // requested d3 against the resolved y.
        let flag = self.on_ground || d3 != y && d3 < 0.0;
        if self.step_height > 0.0 && flag && (d2 != x || d4 != z) {
            let (d14, d6, d7) = (x, y, z);
            let axisalignedbb1 = self.bb;
            self.bb = axisalignedbb;
            y = self.step_height as f64;
            let list = get_collision_boxes(w, &self.bb.expand(d2, y, d4));

            let mut axisalignedbb2 = self.bb;
            let axisalignedbb3 = axisalignedbb2.expand(d2, 0.0, d4);
            let mut d8 = y;
            for b in &list {
                d8 = b.calculate_y_offset(&axisalignedbb3, d8);
            }
            axisalignedbb2 = axisalignedbb2.offset(0.0, d8, 0.0);
            let mut d18 = d2;
            for b in &list {
                d18 = b.calculate_x_offset(&axisalignedbb2, d18);
            }
            axisalignedbb2 = axisalignedbb2.offset(d18, 0.0, 0.0);
            let mut d19 = d4;
            for b in &list {
                d19 = b.calculate_z_offset(&axisalignedbb2, d19);
            }
            axisalignedbb2 = axisalignedbb2.offset(0.0, 0.0, d19);

            let mut axisalignedbb4 = self.bb;
            let mut d20 = y;
            for b in &list {
                d20 = b.calculate_y_offset(&axisalignedbb4, d20);
            }
            axisalignedbb4 = axisalignedbb4.offset(0.0, d20, 0.0);
            let mut d21 = d2;
            for b in &list {
                d21 = b.calculate_x_offset(&axisalignedbb4, d21);
            }
            axisalignedbb4 = axisalignedbb4.offset(d21, 0.0, 0.0);
            let mut d22 = d4;
            for b in &list {
                d22 = b.calculate_z_offset(&axisalignedbb4, d22);
            }
            axisalignedbb4 = axisalignedbb4.offset(0.0, 0.0, d22);

            let d23 = d18 * d18 + d19 * d19;
            let d9 = d21 * d21 + d22 * d22;
            if d23 > d9 {
                x = d18;
                z = d19;
                y = -d8;
                self.bb = axisalignedbb2;
            } else {
                x = d21;
                z = d22;
                y = -d20;
                self.bb = axisalignedbb4;
            }
            for b in &list {
                y = b.calculate_y_offset(&self.bb, y);
            }
            self.bb = self.bb.offset(0.0, y, 0.0);
            if d14 * d14 + d7 * d7 >= x * x + z * z {
                x = d14;
                y = d6;
                z = d7;
                self.bb = axisalignedbb1;
            }
        }

        self.reset_position_to_bb();
        self.collided_horizontally = d2 != x || d4 != z;
        self.collided_vertically = d3 != y;
        self.on_ground = self.collided_vertically && d3 < 0.0;
        // DROPPED: updateFallState; nothing takes damage.
        if d2 != x {
            self.motion.x = 0.0;
        }
        if d4 != z {
            self.motion.z = 0.0;
        }
        // Block.onLanded: no generated block overrides it (slime and beds are not placed).
        if d3 != y {
            self.motion.y = 0.0;
        }
        if can_trigger_walking && (!self.on_ground || !sneaking) {
            let d15 = self.pos.x - d10;
            let d17 = self.pos.z - d1;
            let walked = (d15 * d15 + d17 * d17).sqrt() as f32;
            self.distance_walked_modified = (self.distance_walked_modified as f64 + walked as f64 * 0.6) as f32;
            // No ladder is ever placed, so the step distance leaves out y as it does off ladders.
            self.distance_walked_on_step = (self.distance_walked_on_step as f64 + walked as f64 * 0.6) as f32;
            let (bx, mut by, bz) = (floor(self.pos.x), floor(self.pos.y - 0.20000000298023224), floor(self.pos.z));
            let mut under = w.state(bx, by, bz);
            if worldgen::populate::state::mat(under) == worldgen::populate::props::material::AIR {
                let below = w.state(bx, by - 1, bz);
                // BlockFence (85, 113, 188 to 192), BlockWall (139) and BlockFenceGate (107, 183
                // to 187) reach up into the block above.
                if matches!(below >> 4, 85 | 107 | 113 | 139 | 183..=192) {
                    under = below;
                    by -= 1;
                }
            }
            if self.distance_walked_on_step > self.next_step_distance as f32 && worldgen::populate::state::mat(under) != worldgen::populate::props::material::AIR {
                self.next_step_distance = self.distance_walked_on_step as i32 + 1;
                if self.in_water {
                    let m = self.motion;
                    let f1 = ((m.x * m.x * 0.20000000298023224 + m.y * m.y + m.z * m.z * 0.20000000298023224).sqrt() as f32 * 0.35).min(1.0);
                    self.sounds.push(Sound::Swim(f1));
                } else {
                    self.sounds.push(Sound::Step(bx, by, bz));
                }
            }
            // DROPPED: onEntityWalk; no generated block reacts to being walked on.
        }
        // DROPPED: doBlockCollisions; no generated block reacts to being touched.
    }

    /// `EntityPlayerSP.pushOutOfBlocks`: a nudge out of a block the feet ended up inside.
    pub fn push_out_of_blocks(&mut self, w: &impl BlockAccess, x: f64, y: f64, z: f64) {
        let (bx, by, bz) = (floor(x), floor(y), floor(z));
        let d0 = x - bx as f64;
        let d1 = z - bz as f64;
        // isOpenBlockSpace
        let open = |x: i32, z: i32| !is_normal_cube(w.state(x, by, z)) && !is_normal_cube(w.state(x, by + 1, z));
        if !open(bx, bz) {
            let mut i = -1;
            let mut d2 = 9999.0;
            if open(bx - 1, bz) && d0 < d2 {
                d2 = d0;
                i = 0;
            }
            if open(bx + 1, bz) && 1.0 - d0 < d2 {
                d2 = 1.0 - d0;
                i = 1;
            }
            if open(bx, bz - 1) && d1 < d2 {
                d2 = d1;
                i = 4;
            }
            if open(bx, bz + 1) && 1.0 - d1 < d2 {
                i = 5;
            }
            match i {
                0 => self.motion.x = -0.100_000_001_490_116_12,
                1 => self.motion.x = 0.100_000_001_490_116_12,
                4 => self.motion.z = -0.100_000_001_490_116_12,
                5 => self.motion.z = 0.100_000_001_490_116_12,
                _ => {}
            }
        }
    }
}

/// `EntityLivingBase.travel`. `flying` is `capabilities.isFlying`, `speed` is `getAIMoveSpeed`
/// and `jump_factor` is `jumpMovementFactor`; each `move` goes through [`Living::move_self`].
/// DROPPED: elytra, ladders, levitation, and the limb swing the third-person model reads.
#[allow(clippy::too_many_arguments)]
pub fn travel_living(e: &mut impl Living, w: &impl BlockAccess, strafe: f32, vertical: f32, forward: f32, flying: bool, speed: f32, jump_factor: f32) {
    let b = e.body();
    if !b.in_water || flying {
        if !b.is_in_lava(w) || flying {
            let below = |p: &Body| w.state(floor(p.pos.x), floor(p.bb.min_y - 1.0), floor(p.pos.z));
            let mut f6 = 0.91f32;
            if b.on_ground {
                f6 = slipperiness(below(b)) * 0.91;
            }
            let f7 = 0.162_771_36f32 / (f6 * f6 * f6);
            let f8 = if b.on_ground { speed * f7 } else { jump_factor };
            b.move_relative(strafe, vertical, forward, f8);
            f6 = 0.91;
            if b.on_ground {
                f6 = slipperiness(below(b)) * 0.91;
            }
            let m = b.motion;
            e.move_self(w, m.x, m.y, m.z);
            let b = e.body();
            if w.loaded(floor(b.pos.x), floor(b.pos.z)) {
                b.motion.y -= 0.08;
            } else if b.pos.y > 0.0 {
                b.motion.y = -0.1;
            } else {
                b.motion.y = 0.0;
            }
            b.motion.y *= 0.980_000_019_073_486_3;
            b.motion.x *= f6 as f64;
            b.motion.z *= f6 as f64;
        } else {
            let d4 = b.pos.y;
            b.move_relative(strafe, vertical, forward, 0.02);
            let m = b.motion;
            e.move_self(w, m.x, m.y, m.z);
            let b = e.body();
            b.motion *= 0.5;
            b.motion.y -= 0.02;
            if b.collided_horizontally && b.is_offset_position_in_liquid(w, b.motion.x, b.motion.y + 0.600_000_023_841_857_9 - b.pos.y + d4, b.motion.z) {
                b.motion.y = 0.300_000_011_920_928_96;
            }
        }
    } else {
        let d0 = b.pos.y;
        // getWaterSlowDown, and no depth strider.
        let f1 = 0.8f32;
        let f2 = 0.02f32;
        b.move_relative(strafe, vertical, forward, f2);
        let m = b.motion;
        e.move_self(w, m.x, m.y, m.z);
        let b = e.body();
        b.motion.x *= f1 as f64;
        b.motion.y *= 0.800_000_011_920_929;
        b.motion.z *= f1 as f64;
        b.motion.y -= 0.02;
        if b.collided_horizontally && b.is_offset_position_in_liquid(w, b.motion.x, b.motion.y + 0.600_000_023_841_857_9 - b.pos.y + d0, b.motion.z) {
            b.motion.y = 0.300_000_011_920_928_96;
        }
    }
}
