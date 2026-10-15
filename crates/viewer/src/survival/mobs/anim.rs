//! The client-side animation state some mobs keep beside `Living`: the sheep's eat-grass timer
//! (`EntitySheep.sheepTimer`), the chicken's wing flap (`EntityChicken.wingRotation`, `destPos`,
//! `oFlap`, `oFlapSpeed`, `wingRotDelta`), and the wool colors of `EnumDyeColor`.
//!
//! DROPPED: `EntityChicken`'s `motionY *= 0.6` while falling (the viewer never moves a remote mob
//! by its motion, so nothing reads it), the egg timer (server side), and `EntitySheep`'s server
//! side `entityAIEatGrass` timer (`updateAITasks`); the client only sees status 10.

use worldgen::math_helper::sin;

use super::living::Frame;
use crate::textures::Argb;

/// `EntityCreeper.fuseTime`
pub const CREEPER_FUSE: i32 = 30;

/// `handleStatusUpdate` 10 on a sheep: `sheepTimer = 40`.
pub const EAT_GRASS_TICKS: i32 = 40;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Anim {
    /// `EntitySheep.sheepTimer`
    pub sheep_timer: i32,
    pub wing_rotation: f32,
    pub dest_pos: f32,
    pub o_flap_speed: f32,
    pub o_flap: f32,
    pub wing_rot_delta: f32,
    /// `EntityCreeper.timeSinceIgnited` and `lastActiveTime`
    pub ignite_time: i32,
    pub last_ignite: i32,
}

impl Default for Anim {
    fn default() -> Anim {
        Anim { sheep_timer: 0, wing_rotation: 0.0, dest_pos: 0.0, o_flap_speed: 0.0, o_flap: 0.0, wing_rot_delta: 1.0, ignite_time: 0, last_ignite: 0 }
    }
}

impl Anim {
    /// `EntitySheep.onLivingUpdate`'s client branch: `sheepTimer = max(0, sheepTimer - 1)`.
    pub fn tick_sheep(&mut self) {
        self.sheep_timer = (self.sheep_timer - 1).max(0);
    }

    /// `EntityCreeper.onUpdate`: `timeSinceIgnited += creeperState`, floored at 0 and held at
    /// `fuseTime` (the server explodes it there). APPROXIMATION: the creeper state is -1 or 1 on the
    /// wire as `MM_IGNITED`, which is all the client can know; vanilla's client reads the synced
    /// `STATE` value itself and so counts the same way.
    pub fn tick_creeper(&mut self, ignited: bool) {
        self.last_ignite = self.ignite_time;
        self.ignite_time = (self.ignite_time + if ignited { 1 } else { -1 }).clamp(0, CREEPER_FUSE);
    }

    /// `EntityCreeper.getCreeperFlashIntensity`
    pub fn flash_intensity(&self, partial: f32) -> f32 {
        (self.last_ignite as f32 + (self.ignite_time - self.last_ignite) as f32 * partial) / (CREEPER_FUSE - 2) as f32
    }

    /// `EntityChicken.onLivingUpdate` after `super`, the client part.
    pub fn tick_chicken(&mut self, on_ground: bool) {
        self.o_flap = self.wing_rotation;
        self.o_flap_speed = self.dest_pos;
        self.dest_pos = (self.dest_pos as f64 + (if on_ground { -1 } else { 4 }) as f64 * 0.3) as f32;
        self.dest_pos = self.dest_pos.clamp(0.0, 1.0);
        if !on_ground && self.wing_rot_delta < 1.0 {
            self.wing_rot_delta = 1.0;
        }
        self.wing_rot_delta = (self.wing_rot_delta as f64 * 0.9) as f32;
        self.wing_rotation += self.wing_rot_delta * 2.0;
    }

    /// `RenderChicken.handleRotationFloat`: the wing angle the model gets as `ageInTicks`.
    pub fn wing_angle(&self, partial: f32) -> f32 {
        let f = self.o_flap + (self.wing_rotation - self.o_flap) * partial;
        let f1 = self.o_flap_speed + (self.dest_pos - self.o_flap_speed) * partial;
        (sin(f) + 1.0) * f1
    }

    /// `EntitySheep.getHeadRotationPointY`
    pub fn head_rotation_point_y(&self, partial: f32) -> f32 {
        let t = self.sheep_timer;
        if t <= 0 {
            0.0
        } else if (4..=36).contains(&t) {
            1.0
        } else if t < 4 {
            (t as f32 - partial) / 4.0
        } else {
            -((t - 40) as f32 - partial) / 4.0
        }
    }

    /// `EntitySheep.getHeadRotationAngleX`; `pitch` is the entity's `rotationPitch` (not interpolated).
    pub fn head_rotation_angle_x(&self, partial: f32, pitch: f32) -> f32 {
        let t = self.sheep_timer;
        if t > 4 && t <= 36 {
            let f = ((t - 4) as f32 - partial) / 32.0;
            0.628_318_55 + 0.219_911_49 * sin(f * 28.7)
        } else if t > 0 {
            0.628_318_55
        } else {
            pitch * 0.017_453_292
        }
    }

    /// The sheep's two `setLivingAnimations` values set on a frame: `rotationPointY = 6 + y * 9`
    /// (part 0's point shifts by `y * 9`) and the head's X angle.
    pub fn sheep_frame(&self, frame: &mut Frame, partial: f32, pitch: f32) {
        frame.head_dy = self.head_rotation_point_y(partial) * 9.0;
        frame.head_x = Some(self.head_rotation_angle_x(partial, pitch));
    }
}

/// `EnumDyeColor.getColorValue` by metadata (0 white to 15 black).
const DYE_VALUES: [u32; 16] = [
    16383998, 16351261, 13061821, 3847130, 16701501, 8439583, 15961002, 4673362, 10329495, 1481884, 8991416, 3949738, 8606770, 6192150, 11546150, 1908001,
];

/// `EntitySheep.getDyeRgb(EnumDyeColor.byMetadata(meta))`: the dye's color components times 0.75,
/// and white fixed at 0.9019608.
pub fn fleece_rgb(meta: u8) -> [f32; 3] {
    let meta = if meta < 16 { meta } else { 0 };
    if meta == 0 {
        return [0.901_960_8; 3];
    }
    let v = DYE_VALUES[meta as usize];
    let c = |shift: u32| ((v >> shift) & 255) as f32 / 255.0 * 0.75;
    [c(16), c(8), c(0)]
}

/// `GlStateManager.color(r, g, b)` over a texture, baked into a copy: each channel scaled, alpha kept.
pub fn tinted(img: &Argb, rgb: [f32; 3]) -> Argb {
    let pixels = img
        .pixels
        .iter()
        .map(|&p| {
            let ch = |shift: u32, k: f32| ((((p >> shift) & 255) as f32 * k).round() as u32).min(255) << shift;
            (p & 0xff00_0000) | ch(16, rgb[0]) | ch(8, rgb[1]) | ch(0, rgb[2])
        })
        .collect();
    Argb { width: img.width, height: img.height, pixels }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn white_wool_is_a_fixed_light_gray_and_the_dyes_are_three_quarters_of_theirs() {
        assert_eq!(fleece_rgb(0), [0.901_960_8; 3]);
        // red 11546150 = 0xB02E26
        let r = fleece_rgb(14);
        assert!((r[0] - 0xB0 as f32 / 255.0 * 0.75).abs() < 1e-6);
        assert!((r[1] - 0x2E as f32 / 255.0 * 0.75).abs() < 1e-6);
        assert!((r[2] - 0x26 as f32 / 255.0 * 0.75).abs() < 1e-6);
        // black 1908001 = 0x1D1D21
        assert!((fleece_rgb(15)[2] - 0x21 as f32 / 255.0 * 0.75).abs() < 1e-6);
        assert_eq!(fleece_rgb(99), fleece_rgb(0), "byMetadata wraps an unknown value to white");
    }

    #[test]
    fn the_wool_table_has_sixteen_distinct_colors() {
        let mut all: Vec<_> = (0..16).map(|m| fleece_rgb(m).map(|c| (c * 1000.0) as i32)).collect();
        all.sort();
        all.dedup();
        assert_eq!(all.len(), 16);
    }

    #[test]
    fn tinting_scales_the_channels_and_keeps_alpha() {
        let img = Argb { width: 2, height: 1, pixels: vec![0xff80_c8ff, 0x0010_2030] };
        let t = tinted(&img, [0.5, 1.0, 0.0]);
        assert_eq!(t.pixels, vec![0xff40_c800, 0x0008_2000]);
    }

    #[test]
    fn the_chicken_flaps_faster_in_the_air_and_settles_on_the_ground() {
        let mut a = Anim::default();
        a.tick_chicken(false);
        assert!((a.dest_pos - 1.0).abs() < 1e-6, "0 + 4 * 0.3 clamps to 1");
        assert!((a.wing_rot_delta - 0.9).abs() < 1e-6);
        assert!((a.wing_rotation - 1.8).abs() < 1e-5);
        assert_eq!((a.o_flap, a.o_flap_speed), (0.0, 0.0));
        a.tick_chicken(false);
        assert_eq!(a.wing_rot_delta, 0.9, "airborne resets the delta to 1 before the 0.9");
        assert!((a.o_flap - 1.8).abs() < 1e-5 && (a.wing_rotation - 3.6).abs() < 1e-5);
        for _ in 0..4 {
            a.tick_chicken(true);
        }
        assert!(a.dest_pos == 0.0, "-0.3 a tick back to 0: {}", a.dest_pos);
        assert!(a.wing_rot_delta < 0.9 * 0.9 * 0.9);
    }

    #[test]
    fn the_wing_angle_is_zero_on_the_ground_and_swings_between_zero_and_two_in_flight() {
        let mut a = Anim::default();
        assert_eq!(a.wing_angle(0.5), 0.0);
        let mut max = 0.0f32;
        for _ in 0..40 {
            a.tick_chicken(false);
            let w = a.wing_angle(0.3);
            assert!((0.0..=2.0).contains(&w), "{w}");
            max = max.max(w);
        }
        assert!(max > 1.5);
        // The interpolation: halfway between the previous and current angle and speed.
        let b = Anim { o_flap: 0.0, wing_rotation: 1.0, o_flap_speed: 0.5, dest_pos: 1.0, ..Anim::default() };
        assert!((b.wing_angle(0.5) - (sin(0.5) + 1.0) * 0.75).abs() < 1e-6);
    }

    #[test]
    fn the_sheep_head_dips_over_four_ticks_stays_down_and_comes_back() {
        let mut a = Anim::default();
        assert_eq!((a.head_rotation_point_y(0.0), a.head_rotation_angle_x(0.0, 10.0)), (0.0, 10.0 * 0.017_453_292));
        a.sheep_timer = 40;
        assert_eq!(a.head_rotation_point_y(0.0), 0.0, "(40 - 40 - 0) / 4");
        a.sheep_timer = 38;
        assert_eq!(a.head_rotation_point_y(0.0), 0.5);
        a.sheep_timer = 36;
        assert_eq!(a.head_rotation_point_y(0.7), 1.0);
        a.sheep_timer = 4;
        assert_eq!(a.head_rotation_point_y(0.7), 1.0);
        a.sheep_timer = 2;
        assert_eq!(a.head_rotation_point_y(0.0), 0.5);
        // Down: the angle is 36 degrees for the first and last 4 ticks, a nod in between.
        a.sheep_timer = 39;
        assert_eq!(a.head_rotation_angle_x(0.0, 99.0), 0.628_318_55);
        a.sheep_timer = 20;
        let x = a.head_rotation_angle_x(0.0, 99.0);
        assert!((x - (0.628_318_55 + 0.219_911_49 * sin(16.0 / 32.0 * 28.7))).abs() < 1e-6);
        a.sheep_timer = 3;
        assert_eq!(a.head_rotation_angle_x(0.0, 99.0), 0.628_318_55);
    }

    #[test]
    fn the_eat_timer_counts_down_to_zero() {
        let mut a = Anim { sheep_timer: EAT_GRASS_TICKS, ..Anim::default() };
        for _ in 0..41 {
            a.tick_sheep();
        }
        assert_eq!(a.sheep_timer, 0);
    }
}
