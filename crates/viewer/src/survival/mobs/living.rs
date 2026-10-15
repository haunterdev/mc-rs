//! The client half of a server-tracked mob: `EntityLivingBase` as `NetHandlerPlayClient` drives it
//! (`handleSpawnMob`, `handleEntityTeleport`, `handleEntityHeadLook`, `handleEntityVelocity`) and
//! as it ticks (`onEntityUpdate`'s previous values, `onLivingUpdate`'s 3 tick interpolation, the
//! limb swing block, the body turn of `onUpdate` and `updateDistance`), plus the values
//! `RenderLivingBase.doRender` reads from it.
//!
//! The hurt and death state is `handleStatusUpdate`'s: status 2 (and 36, 37) sets `hurtTime`, status
//! 3 sets the health to 0 so `onDeathUpdate` counts `deathTime` to 20, where `setDead` removes
//! the mob.
//!
//! The arm swing is `S_ANIMATION` 0 (`swingArm`), counted by `EntityMob.onLivingUpdate`'s
//! `updateArmSwingProgress` and read as `Frame::swing`.
//!
//! DROPPED: haste and mining fatigue on the swing length, the riding
//! branches (no mounts), `hurtResistantTime` (nothing on the client reads it), the 20 smoke
//! puffs at `deathTime` 20 (`EXPLOSION_NORMAL`), `EntityTracker.updateServerPosition` (the
//! server's own fixed-point copy of the position), and the mob's own `travel`: a remote entity
//! is not `isServerWorld`, so nothing moves it between packets but the interpolation.

use bevy::math::DVec3;
use proto::{EntityMove, SpawnMob};
use worldgen::math_helper::sqrt_f;

/// `MathHelper.wrapDegrees(float)`
pub fn wrap_degrees(value: f32) -> f32 {
    let mut f = value % 360.0;
    if f >= 180.0 {
        f -= 360.0;
    }
    if f < -180.0 {
        f += 360.0;
    }
    f
}

/// `MathHelper.wrapDegrees(double)`
fn wrap_degrees_d(value: f64) -> f64 {
    let mut d = value % 360.0;
    if d >= 180.0 {
        d -= 360.0;
    }
    if d < -180.0 {
        d += 360.0;
    }
    d
}

/// `EntityLivingBase.maxHurtTime` after a hurt status.
pub const MAX_HURT_TIME: i32 = 10;
/// `getArmSwingAnimationEnd` without a potion
pub const ARM_SWING_TICKS: i32 = 6;
/// `onDeathUpdate`: the tick `deathTime` reaches and the entity is removed.
pub const DEATH_TICKS: i32 = 20;

/// `RenderLivingBase.applyRotations`' lie-down: `sqrt((deathTime + partial - 1) / 20 * 1.6)`
/// capped at 1, times `getDeathMaxRotation` 90, as the Z rotation in degrees.
pub fn death_rotation(death_time: i32, partial: f32) -> f32 {
    if death_time <= 0 {
        return 0.0;
    }
    let f = sqrt_f((death_time as f32 + partial - 1.0) / 20.0 * 1.6);
    f.min(1.0) * 90.0
}

/// `RenderLivingBase.setBrightness`'s overlay: red at 0.3 while `hurtTime > 0 || deathTime > 0`.
pub fn hurt_overlay(hurt_time: i32, death_time: i32) -> f32 {
    if hurt_time > 0 || death_time > 0 {
        0.3
    } else {
        0.0
    }
}

/// `RenderLivingBase.interpolateRotation`
pub fn interpolate_rotation(prev: f32, cur: f32, partial: f32) -> f32 {
    let mut f = cur - prev;
    while f < -180.0 {
        f += 360.0;
    }
    while f >= 180.0 {
        f -= 360.0;
    }
    prev + partial * f
}

/// `handleEntityTeleport`'s snap threshold: a move smaller than this on every axis only turns
/// the entity (zero increments), anything else takes three.
const SNAP: DVec3 = DVec3::new(0.03125, 0.015625, 0.03125);

/// The fields of an `EntityLivingBase` the client keeps for a mob.
#[derive(Clone, Debug)]
pub struct Living {
    pub pos: DVec3,
    pub prev_pos: DVec3,
    pub motion: DVec3,
    /// `rotationYaw`, `rotationPitch` and their previous tick values
    pub yaw: f32,
    pub prev_yaw: f32,
    pub pitch: f32,
    pub prev_pitch: f32,
    /// `rotationYawHead`
    pub head_yaw: f32,
    pub prev_head_yaw: f32,
    /// `renderYawOffset`
    pub body_yaw: f32,
    pub prev_body_yaw: f32,
    pub limb_swing: f32,
    pub limb_amount: f32,
    pub prev_limb_amount: f32,
    /// `ticksExisted`
    pub age: u32,
    /// `newPosRotationIncrements` and the `interpTarget*` fields
    pub increments: i32,
    target_pos: DVec3,
    target_yaw: f64,
    target_pitch: f64,
    pub on_ground: bool,
    /// `hurtTime`, counting down from `MAX_HURT_TIME`
    pub hurt_time: i32,
    /// `getHealth() <= 0`, set by status 3
    pub health_zero: bool,
    /// `deathTime`
    pub death_time: i32,
    /// `isDead`: `setDead` at `deathTime` 20, so the mob is taken out of the world
    pub dead: bool,
    /// `isSwingInProgress`, `swingProgressInt`, `swingProgress` and `prevSwingProgress`
    pub swinging: bool,
    pub swing_int: i32,
    pub swing_progress: f32,
    pub prev_swing_progress: f32,
}

/// What `RenderLivingBase.doRender` hands the model for one frame.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Frame {
    /// `f`: the interpolated `renderYawOffset`, which turns the body
    pub body_yaw: f32,
    /// `f2`: `netHeadYaw`, the head's turn relative to the body
    pub net_head_yaw: f32,
    /// `f7`: the interpolated pitch
    pub head_pitch: f32,
    /// `f8`: `ticksExisted + partialTicks`
    pub age: f32,
    /// `f6`
    pub limb_swing: f32,
    /// `f5`
    pub limb_amount: f32,
    /// `ModelSheep`'s `head.rotationPointY` shift (`getHeadRotationPointY * 9`) added to part 0's
    /// point; 0 for every other kind
    pub head_dy: f32,
    /// `ModelSheep.headRotationAngleX`, which replaces the head's X rotation; `None` for every other kind
    pub head_x: Option<f32>,
    /// `ModelBase.swingProgress`: `getSwingProgress(partial)`
    pub swing: f32,
    /// `MobMeta::flags`: what a model reads off its entity (`isArmsRaised`, `isSwingingArms`)
    pub flags: u8,
    /// The main hand holds a bow (`ModelSkeleton.setLivingAnimations`)
    pub bow: bool,
}

impl Living {
    /// `handleSpawnMob`: `setPositionAndRotation` (previous values follow), `renderYawOffset` and
    /// `rotationYawHead` from the packet's head yaw, the motion as given.
    pub fn spawned(s: &SpawnMob) -> Living {
        let pos = DVec3::from_array(s.pos);
        Living {
            pos,
            prev_pos: pos,
            motion: DVec3::from_array(s.motion.map(f64::from)),
            yaw: s.yaw % 360.0,
            prev_yaw: s.yaw % 360.0,
            pitch: s.pitch % 360.0,
            prev_pitch: s.pitch % 360.0,
            head_yaw: s.head_yaw,
            // DROPPED: the first frame before the first tick interpolates from 0 in the original.
            prev_head_yaw: s.head_yaw,
            body_yaw: s.head_yaw,
            prev_body_yaw: s.head_yaw,
            limb_swing: 0.0,
            limb_amount: 0.0,
            prev_limb_amount: 0.0,
            age: 0,
            increments: 0,
            target_pos: pos,
            target_yaw: 0.0,
            target_pitch: 0.0,
            on_ground: false,
            hurt_time: 0,
            health_zero: false,
            death_time: 0,
            dead: false,
            swinging: false,
            swing_int: 0,
            swing_progress: 0.0,
            prev_swing_progress: 0.0,
        }
    }

    /// `EntityLivingBase.swingArm(MAIN_HAND)`, which `handleAnimation` 0 calls.
    pub fn swing_arm(&mut self) {
        if !self.swinging || self.swing_int >= ARM_SWING_TICKS / 2 || self.swing_int < 0 {
            self.swing_int = -1;
            self.swinging = true;
        }
    }

    /// `EntityLivingBase.updateArmSwingProgress` (no haste or mining fatigue on a remote mob).
    fn update_arm_swing_progress(&mut self) {
        if self.swinging {
            self.swing_int += 1;
            if self.swing_int >= ARM_SWING_TICKS {
                self.swing_int = 0;
                self.swinging = false;
            }
        } else {
            self.swing_int = 0;
        }
        self.swing_progress = self.swing_int as f32 / ARM_SWING_TICKS as f32;
    }

    /// `EntityLivingBase.getSwingProgress`
    pub fn swing_at(&self, partial: f32) -> f32 {
        let mut f = self.swing_progress - self.prev_swing_progress;
        if f < 0.0 {
            f += 1.0;
        }
        self.prev_swing_progress + f * partial
    }

    /// `handleStatusUpdate` 2, 36 and 37: `limbSwingAmount` 1.5, `hurtTime = maxHurtTime`.
    pub fn hurt(&mut self) {
        self.limb_amount = 1.5;
        self.hurt_time = MAX_HURT_TIME;
    }

    /// `handleStatusUpdate` 3: `setHealth(0)`, which starts `onDeathUpdate`.
    pub fn die(&mut self) {
        self.health_zero = true;
    }

    /// `handleEntityTeleport` (`m.teleport`) or `handleEntityMovement` (the relative packets), and
    /// `handleEntityHeadLook`, for one `S_ENTITY_MOVE` entry. Only the teleport packet snaps a
    /// target closer than `SNAP` to no increments; the relative packets always take three, so a
    /// slow mover's tiny steps still glide instead of stepping.
    pub fn moved(&mut self, m: &EntityMove) {
        let to = DVec3::from_array(m.pos);
        let near = m.teleport && (self.pos.x - to.x).abs() < SNAP.x && (self.pos.y - to.y).abs() < SNAP.y && (self.pos.z - to.z).abs() < SNAP.z;
        // setPositionAndRotationDirect: the target, and how many ticks to take.
        if near {
            self.set_direct(self.pos, m.yaw, m.pitch, 0);
        } else {
            self.set_direct(to, m.yaw, m.pitch, 3);
        }
        self.on_ground = m.on_ground;
        // setRotationYawHead: set at once, the render interpolates it from the previous tick.
        self.head_yaw = m.head_yaw;
    }

    fn set_direct(&mut self, pos: DVec3, yaw: f32, pitch: f32, increments: i32) {
        self.target_pos = pos;
        self.target_yaw = yaw as f64;
        self.target_pitch = pitch as f64;
        self.increments = increments;
    }

    /// `handleEntityVelocity` -> `setVelocity`
    pub fn set_velocity(&mut self, motion: [f32; 3]) {
        self.motion = DVec3::from_array(motion.map(f64::from));
    }

    /// One client tick: `Entity.onUpdate` and `EntityLivingBase.onUpdate` for a remote mob.
    pub fn tick(&mut self) {
        self.age += 1;
        // onEntityUpdate: the previous values.
        self.prev_pos = self.pos;
        self.prev_yaw = self.yaw;
        self.prev_pitch = self.pitch;
        self.prev_body_yaw = self.body_yaw;
        self.prev_head_yaw = self.head_yaw;
        self.prev_swing_progress = self.swing_progress;
        // onEntityUpdate: hurtTime counts down, a mob at 0 health runs onDeathUpdate.
        if self.hurt_time > 0 {
            self.hurt_time -= 1;
        }
        if self.health_zero {
            self.death_time += 1;
            if self.death_time == DEATH_TICKS {
                self.dead = true;
            }
        }
        // onLivingUpdate: the interpolation over `newPosRotationIncrements` ticks.
        if self.increments > 0 {
            let n = self.increments as f64;
            let p = self.pos + (self.target_pos - self.pos) / n;
            let d3 = wrap_degrees_d(self.target_yaw - self.yaw as f64);
            self.yaw = (self.yaw as f64 + d3 / n) as f32;
            self.pitch = (self.pitch as f64 + (self.target_pitch - self.pitch as f64) / n) as f32;
            self.increments -= 1;
            self.pos = p;
            // setRotation
            self.yaw %= 360.0;
            self.pitch %= 360.0;
        } else {
            // !isServerWorld: the motion decays and snaps to zero.
            self.motion *= 0.98;
        }
        for m in [&mut self.motion.x, &mut self.motion.y, &mut self.motion.z] {
            if m.abs() < 0.003 {
                *m = 0.0;
            }
        }
        // EntityMob.onLivingUpdate
        self.update_arm_swing_progress();
        // The limb block: how far it moved this tick (not EntityFlying, so no y).
        self.prev_limb_amount = self.limb_amount;
        let (dx, dz) = (self.pos.x - self.prev_pos.x, self.pos.z - self.prev_pos.z);
        let f10 = (sqrt_f((dx * dx + dz * dz) as f32) * 4.0).min(1.0);
        self.limb_amount += (f10 - self.limb_amount) * 0.4;
        self.limb_swing += self.limb_amount;
        // onUpdate: the body turns toward the way it moves.
        let f3 = (dx * dx + dz * dz) as f32;
        let mut f4 = self.body_yaw;
        if f3 > 0.002_500_000_2 {
            let f1 = (dz as f32).atan2(dx as f32) * 57.295_776 - 90.0;
            let f2 = (wrap_degrees(self.yaw) - f1).abs();
            f4 = if 95.0 < f2 && f2 < 265.0 { f1 - 180.0 } else { f1 };
        }
        if self.swing_progress > 0.0 {
            f4 = self.yaw;
        }
        self.update_distance(f4);
        // The range checks keep each previous value within half a turn of the current one.
        for (cur, prev) in [
            (self.yaw, &mut self.prev_yaw),
            (self.body_yaw, &mut self.prev_body_yaw),
            (self.pitch, &mut self.prev_pitch),
            (self.head_yaw, &mut self.prev_head_yaw),
        ] {
            while cur - *prev < -180.0 {
                *prev -= 360.0;
            }
            while cur - *prev >= 180.0 {
                *prev += 360.0;
            }
        }
    }

    /// `EntityLivingBase.updateDistance`, without the returned stride (only `movedDistance` reads it).
    fn update_distance(&mut self, target: f32) {
        let f = wrap_degrees(target - self.body_yaw);
        self.body_yaw += f * 0.3;
        let mut f1 = wrap_degrees(self.yaw - self.body_yaw);
        f1 = f1.clamp(-75.0, 75.0);
        self.body_yaw = self.yaw - f1;
        if f1 * f1 > 2500.0 {
            self.body_yaw += f1 * 0.2;
        }
    }

    /// The feet between the last two ticks.
    pub fn interp_pos(&self, partial: f32) -> DVec3 {
        self.prev_pos + (self.pos - self.prev_pos) * partial as f64
    }

    /// `RenderLivingBase.doRender`'s numbers for the model. `child` multiplies the limb swing by 3.
    pub fn frame(&self, partial: f32, child: bool) -> Frame {
        let body_yaw = interpolate_rotation(self.prev_body_yaw, self.body_yaw, partial);
        let head = interpolate_rotation(self.prev_head_yaw, self.head_yaw, partial);
        let mut limb_swing = self.limb_swing - self.limb_amount * (1.0 - partial);
        if child {
            limb_swing *= 3.0;
        }
        Frame {
            body_yaw,
            net_head_yaw: head - body_yaw,
            head_pitch: self.prev_pitch + (self.pitch - self.prev_pitch) * partial,
            age: self.age as f32 + partial,
            limb_swing,
            limb_amount: (self.prev_limb_amount + (self.limb_amount - self.prev_limb_amount) * partial).min(1.0),
            head_dy: 0.0,
            head_x: None,
            swing: self.swing_at(partial),
            flags: 0,
            bow: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(x: f64) -> Living {
        Living::spawned(&SpawnMob { pos: [x, 64.0, 0.0], ..Default::default() })
    }

    fn walk_to(x: f64, yaw: f32) -> EntityMove {
        EntityMove { pos: [x, 64.0, 0.0], yaw, head_yaw: yaw, ..Default::default() }
    }

    #[test]
    fn a_move_takes_three_ticks_in_equal_shares_of_what_is_left() {
        let mut m = at(0.0);
        m.moved(&walk_to(3.0, 0.0));
        let mut xs = Vec::new();
        for _ in 0..4 {
            m.tick();
            xs.push(m.pos.x);
        }
        assert_eq!(xs, [1.0, 2.0, 3.0, 3.0]);
        assert_eq!(m.increments, 0);
        assert_eq!(m.prev_pos.x, 3.0);
    }

    #[test]
    fn a_new_move_mid_flight_restarts_the_three_ticks() {
        let mut m = at(0.0);
        m.moved(&walk_to(3.0, 0.0));
        m.tick();
        m.moved(&walk_to(4.0, 0.0));
        assert_eq!(m.increments, 3);
        m.tick();
        assert!((m.pos.x - 2.0).abs() < 1e-12, "(4 - 1) / 3 further: {}", m.pos.x);
    }

    #[test]
    fn a_tiny_relative_move_still_takes_three_increments() {
        let mut m = at(5.0);
        m.moved(&walk_to(5.02, 90.0));
        assert_eq!(m.increments, 3);
        m.tick();
        assert!((m.pos.x - (5.0 + 0.02 / 3.0)).abs() < 1e-12, "{}", m.pos.x);
    }

    /// A mob walking at a steady speed gets a position packet every third tick (`tracker_interval`
    /// 3): the render moves it the same amount every tick, and between ticks by the partial tick.
    #[test]
    fn a_steady_walker_renders_in_even_steps() {
        let v = 0.2;
        let mut m = at(0.0);
        let mut seen = Vec::new();
        for packet in 1..=3 {
            m.moved(&walk_to(v * 3.0 * packet as f64, 0.0));
            for _ in 0..3 {
                m.tick();
                seen.push(m.interp_pos(1.0).x);
            }
        }
        let steps: Vec<f64> = seen.windows(2).map(|w| w[1] - w[0]).collect();
        for s in &steps {
            assert!((s - v).abs() < 1e-9, "{steps:?}");
        }
        // Within a tick the render moves linearly between prevPos and pos.
        let (a, b) = (m.interp_pos(0.0).x, m.interp_pos(1.0).x);
        assert!((m.interp_pos(0.5).x - (a + b) / 2.0).abs() < 1e-9);
    }

    /// Three packets one tick apart (a mob that is air borne or has dirty metadata is sent every
    /// tick): each restarts the three tick glide, the steps grow towards the walking speed and
    /// never go backwards or jump.
    #[test]
    fn packets_one_tick_apart_glide_without_jumps() {
        let v = 0.2;
        let mut m = at(0.0);
        let mut last = 0.0;
        let mut steps = Vec::new();
        for packet in 1..=3 {
            m.moved(&walk_to(v * packet as f64, 0.0));
            m.tick();
            let x = m.interp_pos(1.0).x;
            steps.push(x - last);
            last = x;
        }
        assert!(steps.windows(2).all(|w| w[1] > w[0] && w[1] < v + 1e-9), "{steps:?}");
        assert!(steps.iter().all(|s| *s > 0.0 && *s < v));
    }

    #[test]
    fn a_tiny_move_takes_no_increments_and_keeps_the_position() {
        let mut m = at(5.0);
        m.moved(&EntityMove { teleport: true, ..walk_to(5.02, 90.0) });
        assert_eq!(m.increments, 0);
        m.tick();
        assert_eq!(m.pos.x, 5.0);
        assert_eq!(m.yaw, 0.0, "setPositionAndRotationDirect with zero increments leaves the yaw too");
    }

    #[test]
    fn yaw_takes_the_short_way_round() {
        let mut m = Living::spawned(&SpawnMob { yaw: 350.0, ..Default::default() });
        m.moved(&EntityMove { pos: [1.0, 0.0, 0.0], yaw: 10.0, ..Default::default() });
        for _ in 0..3 {
            m.tick();
        }
        assert!((wrap_degrees(m.yaw - 10.0)).abs() < 1e-3, "{}", m.yaw);
        assert!(m.prev_yaw < 20.0 && m.prev_yaw > -20.0 || m.prev_yaw > 330.0, "{}", m.prev_yaw);
    }

    #[test]
    fn head_yaw_is_set_at_once_and_interpolated_by_the_render() {
        let mut m = at(0.0);
        m.moved(&EntityMove { pos: [0.0, 64.0, 0.0], head_yaw: 80.0, ..Default::default() });
        assert_eq!(m.head_yaw, 80.0);
        m.tick();
        // prev 80 -> 80 after the tick, so the render sees 80 against a body at 0.
        let f = m.frame(0.5, false);
        assert_eq!(f.net_head_yaw, 80.0 - f.body_yaw);
    }

    #[test]
    fn interpolate_rotation_wraps() {
        assert_eq!(interpolate_rotation(170.0, -170.0, 0.5), 180.0);
        assert_eq!(interpolate_rotation(0.0, 90.0, 0.25), 22.5);
        assert_eq!(interpolate_rotation(-179.0, 179.0, 1.0), -181.0);
    }

    #[test]
    fn limb_swing_follows_speed_and_decays_when_still() {
        let mut m = at(0.0);
        // A quarter block a tick is f10 = 1: the amount closes 40% of the gap each tick.
        let mut expect = 0.0f32;
        for i in 0..5 {
            m.moved(&walk_to(0.25 * (i + 1) as f64 * 3.0, 0.0));
            for _ in 0..3 {
                m.tick();
                expect += (1.0 - expect) * 0.4;
                assert!((m.limb_amount - expect).abs() < 1e-5, "{} vs {expect}", m.limb_amount);
            }
        }
        let swing = m.limb_swing;
        assert!(swing > 5.0);
        let top = m.limb_amount;
        for _ in 0..10 {
            m.tick();
        }
        assert!(m.limb_amount < top * 0.01, "idle: {}", m.limb_amount);
        assert!(m.limb_swing > swing, "limbSwing only grows by limbSwingAmount");
    }

    #[test]
    fn limb_swing_in_the_frame_is_back_dated_by_the_partial_tick() {
        let mut m = at(0.0);
        m.moved(&walk_to(0.75, 0.0));
        m.tick();
        m.tick();
        let (swing, amount) = (m.limb_swing, m.limb_amount);
        let f = m.frame(0.25, false);
        assert!((f.limb_swing - (swing - amount * 0.75)).abs() < 1e-6);
        assert!(f.limb_amount <= amount && f.limb_amount >= m.prev_limb_amount);
        assert!((m.frame(0.25, true).limb_swing - 3.0 * f.limb_swing).abs() < 1e-5, "a child swings 3 times faster");
    }

    #[test]
    fn limb_amount_is_capped_at_one() {
        let mut m = at(0.0);
        m.prev_limb_amount = 2.0;
        m.limb_amount = 2.0;
        assert_eq!(m.frame(0.5, false).limb_amount, 1.0);
    }

    #[test]
    fn the_body_turns_to_the_way_it_walks_and_stays_within_75_degrees_of_the_head() {
        let mut m = at(0.0);
        // Moving along +x is yaw -90 (atan2(0, 1) - 90).
        m.moved(&EntityMove { pos: [3.0, 64.0, 0.0], yaw: -90.0, head_yaw: -90.0, ..Default::default() });
        for _ in 0..3 {
            m.tick();
        }
        assert!(m.body_yaw < 0.0 && m.body_yaw > -90.0, "turning toward -90: {}", m.body_yaw);
        // Look far off to the side: the body is held within 75 degrees of the yaw.
        let mut m = at(0.0);
        m.yaw = 170.0;
        m.prev_yaw = 170.0;
        for _ in 0..10 {
            m.tick();
        }
        assert!(wrap_degrees(m.yaw - m.body_yaw).abs() <= 75.0 + 1e-3);
    }

    #[test]
    fn the_hurt_overlay_lasts_ten_ticks_from_the_status() {
        let mut m = at(0.0);
        assert_eq!(hurt_overlay(m.hurt_time, m.death_time), 0.0);
        m.hurt();
        assert_eq!(m.limb_amount, 1.5);
        for n in 0..10 {
            assert_eq!(hurt_overlay(m.hurt_time, m.death_time), 0.3, "tick {n} still red");
            m.tick();
        }
        assert_eq!(m.hurt_time, 0);
        assert_eq!(hurt_overlay(m.hurt_time, m.death_time), 0.0, "clear after the tenth tick");
        // A second hit restarts the ten ticks.
        m.hurt();
        m.tick();
        m.hurt();
        assert_eq!(m.hurt_time, 10);
    }

    #[test]
    fn a_dying_mob_counts_to_twenty_stays_red_and_is_then_removed() {
        let mut m = at(0.0);
        m.tick();
        assert_eq!(m.death_time, 0, "alive mobs do not count");
        m.die();
        for n in 1..20 {
            m.tick();
            assert_eq!(m.death_time, n);
            assert!(!m.dead);
            assert_eq!(hurt_overlay(m.hurt_time, m.death_time), 0.3, "red the whole fall");
        }
        m.tick();
        assert!(m.dead && m.death_time == DEATH_TICKS);
    }

    #[test]
    fn the_death_fall_follows_the_square_root_curve_to_ninety_degrees() {
        assert_eq!(death_rotation(0, 0.5), 0.0, "alive: no tilt");
        assert_eq!(death_rotation(1, 0.0), 0.0, "the first tick starts at zero");
        // deathTime 1 + partial 1: sqrt(1 / 20 * 1.6) = 0.2828...
        assert!((death_rotation(1, 1.0) - 25.455_845).abs() < 1e-3, "{}", death_rotation(1, 1.0));
        // deathTime 6, partial 0.5: sqrt(5.5 / 20 * 1.6) = sqrt(0.44)
        assert!((death_rotation(6, 0.5) - 0.44f32.sqrt() * 90.0).abs() < 1e-3);
        // (deathTime + partial - 1) / 20 * 1.6 passes 1 at 12.5 ticks; from there it lies flat.
        assert!(death_rotation(12, 0.0) < 90.0);
        assert_eq!(death_rotation(13, 1.0), 90.0);
        assert_eq!(death_rotation(19, 0.9), 90.0);
        let mut last = 0.0;
        for t in 1..=12 {
            let r = death_rotation(t, 0.0);
            assert!(r >= last, "never rises back up: {t}");
            last = r;
        }
    }

    #[test]
    fn velocity_decays_to_zero() {
        let mut m = at(0.0);
        m.set_velocity([0.1, 0.0, 0.0]);
        for _ in 0..200 {
            m.tick();
        }
        assert_eq!(m.motion, DVec3::ZERO);
    }
}
