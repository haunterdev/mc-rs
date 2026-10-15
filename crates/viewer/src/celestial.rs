//! Time of day, sky, fog and cloud colors: the client-side `World` and `WorldProvider` functions
//! the sky, the fog and the lightmap are computed from. Rain, thunder and lightning are always
//! zero here (no weather), so their branches are dropped where they would multiply by one.

use worldgen::math_helper::{cos, sin};

/// `WorldProvider.calculateCelestialAngle`
pub fn celestial_angle(world_time: i64, partial_ticks: f32) -> f32 {
    let i = (world_time % 24000) as i32;
    let mut f = (i as f32 + partial_ticks) / 24000.0 - 0.25;
    if f < 0.0 {
        f += 1.0;
    }
    if f > 1.0 {
        f -= 1.0;
    }
    let f1 = 1.0 - (((f as f64 * std::f64::consts::PI).cos() + 1.0) / 2.0) as f32;
    f + (f1 - f) / 3.0
}

/// `World.getCelestialAngleRadians`
pub fn celestial_angle_radians(angle: f32) -> f32 {
    angle * 6.283_185_5
}

/// `WorldProvider.getMoonPhase`
pub fn moon_phase(world_time: i64) -> i32 {
    ((world_time / 24000 % 8 + 8) % 8) as i32
}

/// `World.calculateSkylightSubtracted`
pub fn skylight_subtracted(angle: f32) -> i32 {
    let mut f1 = 1.0 - (cos(angle * 6.283_185_5) * 2.0 + 0.5);
    f1 = f1.clamp(0.0, 1.0);
    f1 = 1.0 - f1;
    // DROPPED: the rain and thunder factors, `1 - strength * 5 / 16` with strength 0.
    f1 = 1.0 - f1;
    (f1 * 11.0) as i32
}

/// `World.getSunBrightness`
pub fn sun_brightness(angle: f32) -> f32 {
    let mut f1 = 1.0 - (cos(angle * 6.283_185_5) * 2.0 + 0.2);
    f1 = f1.clamp(0.0, 1.0);
    f1 = 1.0 - f1;
    // DROPPED: the rain and thunder factors.
    f1 * 0.8 + 0.2
}

/// `World.getStarBrightness`
pub fn star_brightness(angle: f32) -> f32 {
    let mut f1 = 1.0 - (cos(angle * 6.283_185_5) * 2.0 + 0.25);
    f1 = f1.clamp(0.0, 1.0);
    f1 * f1 * 0.5
}

/// `MathHelper.hsvToRGB`
pub fn hsv_to_rgb(hue: f32, saturation: f32, value: f32) -> i32 {
    let i = (hue * 6.0) as i32 % 6;
    let f = hue * 6.0 - i as f32;
    let f1 = value * (1.0 - saturation);
    let f2 = value * (1.0 - f * saturation);
    let f3 = value * (1.0 - (1.0 - f) * saturation);
    let (f4, f5, f6) = match i {
        0 => (value, f3, f1),
        1 => (f2, value, f1),
        2 => (f1, value, f3),
        3 => (f1, f2, value),
        4 => (f3, f1, value),
        5 => (value, f1, f2),
        _ => panic!("Something went wrong when converting from HSV to RGB. Input was {hue}, {saturation}, {value}"),
    };
    let c = |v: f32| ((v * 255.0) as i32).clamp(0, 255);
    c(f4) << 16 | c(f5) << 8 | c(f6)
}

/// `Biome.getSkyColorByTemp`
pub fn sky_color_by_temp(temperature: f32) -> i32 {
    let t = (temperature / 3.0).clamp(-1.0, 1.0);
    hsv_to_rgb(0.622_222_24 - t * 0.05, 0.5 + t * 0.1, 1.0)
}

/// `World.getSkyColor`, given the biome temperature at the view entity's block
/// (`Biome.getTemperature(BlockPos)`).
pub fn sky_color(angle: f32, temperature: f32) -> [f32; 3] {
    let mut f1 = cos(angle * 6.283_185_5) * 2.0 + 0.5;
    f1 = f1.clamp(0.0, 1.0);
    let l = sky_color_by_temp(temperature);
    let ch = |shift: u32| (l >> shift & 255) as f32 / 255.0;
    // DROPPED: the rain, thunder and lightning blends.
    [ch(16) * f1, ch(8) * f1, ch(0) * f1]
}

/// `WorldProvider.getFogColor`
pub fn provider_fog_color(angle: f32) -> [f32; 3] {
    let mut f = cos(angle * 6.283_185_5) * 2.0 + 0.5;
    f = f.clamp(0.0, 1.0);
    let f1 = 0.752_941_2 * (f * 0.94 + 0.06);
    let f2 = 0.847_058_83 * (f * 0.94 + 0.06);
    let f3 = 1.0 * (f * 0.91 + 0.09);
    [f1, f2, f3]
}

/// `WorldProvider.calcSunriseSunsetColors`
pub fn sunrise_colors(angle: f32) -> Option<[f32; 4]> {
    let f1 = cos(angle * 6.283_185_5) - 0.0;
    if (-0.4..=0.4).contains(&f1) {
        let f3 = (f1 - -0.0) / 0.4 * 0.5 + 0.5;
        let mut f4 = 1.0 - (1.0 - sin(f3 * std::f32::consts::PI)) * 0.99;
        f4 *= f4;
        Some([f3 * 0.3 + 0.7, f3 * f3 * 0.7 + 0.2, f3 * f3 * 0.0 + 0.2, f4])
    } else {
        None
    }
}

/// `World.getCloudColour`
pub fn cloud_colour(angle: f32) -> [f32; 3] {
    let mut f1 = cos(angle * 6.283_185_5) * 2.0 + 0.5;
    f1 = f1.clamp(0.0, 1.0);
    // DROPPED: the rain and thunder blends.
    [f1 * 0.9 + 0.1, f1 * 0.9 + 0.1, f1 * 0.85 + 0.15]
}

/// What `EntityRenderer.updateFogColor` needs that is not time of day.
pub struct FogInput {
    pub render_distance: u32,
    /// `World.getSkyColor` at the view entity.
    pub sky: [f32; 3],
    /// `Entity.getLook(partialTicks)`
    pub look: [f32; 3],
    /// The fluid at the eye (`ActiveRenderInfo.getBlockStateAtEntityViewpoint`).
    pub fluid: Fluid,
    /// `fogColor2 + (fogColor1 - fogColor2) * partialTicks`
    pub brightness: f32,
    /// The view entity's interpolated feet y (`lastTickPosY + (posY - lastTickPosY) * pt`).
    pub feet_y: f64,
    /// The Blindness effect's remaining ticks, when active.
    pub blindness: Option<i32>,
    /// `getNightVisionBrightness`, 0 without the effect.
    pub night_vision: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fluid {
    None,
    Water,
    Lava,
}

/// `EntityRenderer.updateFogColor`: the fog color, which is also the clear color.
pub fn fog_color(angle: f32, i: &FogInput) -> [f32; 3] {
    let mut f = 0.25 + 0.75 * i.render_distance as f32 / 32.0;
    f = 1.0 - (f as f64).powf(0.25) as f32;
    let [f1, f2, f3] = i.sky;
    let [mut r, mut g, mut b] = provider_fog_color(angle);
    if i.render_distance >= 4 {
        let d0 = if sin(celestial_angle_radians(angle)) > 0.0 { -1.0 } else { 1.0 };
        // Vec3d.dotProduct with (d0, 0, 0), in doubles, then cast.
        let mut f5 = (i.look[0] as f64 * d0) as f32;
        if f5 < 0.0 {
            f5 = 0.0;
        }
        if f5 > 0.0 {
            if let Some(a) = sunrise_colors(angle) {
                f5 *= a[3];
                r = r * (1.0 - f5) + a[0] * f5;
                g = g * (1.0 - f5) + a[1] * f5;
                b = b * (1.0 - f5) + a[2] * f5;
            }
        }
    }
    r += (f1 - r) * f;
    g += (f2 - g) * f;
    b += (f3 - b) * f;
    // DROPPED: rain and thunder darkening, cloud fog (`hasCloudFog` is always false in 1.12.2).
    match i.fluid {
        // Respiration 0 and no water breathing: f12 = 0.
        Fluid::Water => (r, g, b) = (0.02, 0.02, 0.2),
        Fluid::Lava => (r, g, b) = (0.6, 0.1, 0.0),
        Fluid::None => {}
    }
    r *= i.brightness;
    g *= i.brightness;
    b *= i.brightness;
    // WorldProvider.getVoidFogYFactor for the default world type.
    let mut d1 = i.feet_y * 0.03125;
    // Blindness: the void factor gives way to a fade over the last second.
    if let Some(t) = i.blindness {
        d1 = if t < 20 { d1 * (1.0 - t as f32 / 20.0) as f64 } else { 0.0 };
    }
    if d1 < 1.0 {
        if d1 < 0.0 {
            d1 = 0.0;
        }
        d1 *= d1;
        r = (r as f64 * d1) as f32;
        g = (g as f64 * d1) as f32;
        b = (b as f64 * d1) as f32;
    }
    if i.night_vision > 0.0 {
        let mut f4 = 1.0 / r;
        if f4 > 1.0 / g {
            f4 = 1.0 / g;
        }
        if f4 > 1.0 / b {
            f4 = 1.0 / b;
        }
        let d = i.night_vision;
        r = r * (1.0 - d) + r * f4 * d;
        g = g * (1.0 - d) + g * f4 * d;
        b = b * (1.0 - d) + b * f4 * d;
    }
    // DROPPED: boss fog and anaglyph.
    [r, g, b]
}

/// One tick of the fog brightness smoothing in `EntityRenderer.updateRenderer`: `fog_color1`
/// moves a tenth of the way to the light at the view entity's feet. Returns the new
/// `(fogColor2, fogColor1)`.
pub fn fog_brightness_tick(fog_color1: f32, light_brightness: f32, render_distance: u32) -> (f32, f32) {
    let f4 = render_distance as f32 / 32.0;
    let f2 = light_brightness * (1.0 - f4) + f4;
    (fog_color1, fog_color1 + (f2 - fog_color1) * 0.1)
}

/// `EntityRenderer.setupFog` for the open-air case: `(start, end)` of the linear fog.
/// `start_coords` is -1 for the sky and 0 for terrain and clouds.
pub fn linear_fog(render_distance: u32, start_coords: i32) -> (f32, f32) {
    let f = (render_distance * 16) as f32;
    // DROPPED: `doesXZShowFog` (false in the overworld) and the boss overlay fog.
    if start_coords == -1 {
        (0.0, f)
    } else {
        (f * 0.75, f)
    }
}

/// `EntityRenderer.setupFog` with Blindness: linear fog 5 blocks out, easing to the far plane over
/// the last second. The sky (`start_coords` -1) starts at the eye and ends at 0.8 of it.
pub fn blindness_fog(render_distance: u32, start_coords: i32, ticks: i32) -> (f32, f32) {
    let far = (render_distance * 16) as f32;
    let mut f = 5.0f32;
    if ticks < 20 {
        f = 5.0 + (far - 5.0) * (1.0 - ticks as f32 / 20.0);
    }
    if start_coords == -1 {
        (0.0, f * 0.8)
    } else {
        (f * 0.25, f)
    }
}

/// `BlockLiquid.getLiquidHeightPercent`, in `sim::region`.
pub use sim::region::liquid_height_percent;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn noon_and_midnight() {
        // World time 6000 is noon: the angle is 0 and the sky is at full brightness.
        let noon = celestial_angle(6000, 0.0);
        assert!(noon.abs() < 1e-6);
        assert_eq!(skylight_subtracted(noon), 0);
        assert!((sun_brightness(noon) - 1.0).abs() < 1e-6);
        assert_eq!(star_brightness(noon), 0.0);
        let midnight = celestial_angle(18000, 0.0);
        assert!((midnight - 0.5).abs() < 1e-6);
        assert_eq!(skylight_subtracted(midnight), 11);
        assert!((sun_brightness(midnight) - 0.2).abs() < 1e-6);
        assert!((star_brightness(midnight) - 0.5).abs() < 1e-6);
    }

    #[test]
    fn plains_sky_is_the_familiar_blue() {
        // Plains temperature 0.8 at noon: 0x78A7FF.
        assert_eq!(sky_color_by_temp(0.8), 0x78A7FF);
        let s = sky_color(0.0, 0.8);
        assert!((s[0] - 120.0 / 255.0).abs() < 1e-6 && (s[2] - 1.0).abs() < 1e-6);
    }

    #[test]
    fn hsv_matches_the_six_sectors() {
        assert_eq!(hsv_to_rgb(0.0, 1.0, 1.0), 0xFF0000);
        assert_eq!(hsv_to_rgb(1.0 / 3.0, 1.0, 1.0), 0x00FF00);
        assert_eq!(hsv_to_rgb(2.0 / 3.0, 1.0, 1.0), 0x0000FF);
        assert_eq!(hsv_to_rgb(0.5, 0.0, 0.5), 0x7F7F7F);
    }

    #[test]
    fn sunrise_only_near_the_horizon() {
        assert!(sunrise_colors(0.0).is_none());
        // Sunset: world time 12000 puts the sun at the horizon.
        let a = sunrise_colors(celestial_angle(12000, 0.0)).expect("sunset colors");
        assert!(a[0] > 0.7 && a[3] > 0.0);
    }

    #[test]
    fn fog_is_the_horizon_tint_and_dark_in_the_void() {
        let noon = celestial_angle(6000, 0.0);
        let sky = sky_color(noon, 0.8);
        let mut input = FogInput {
            render_distance: 12,
            sky,
            look: [0.0, 0.0, 1.0],
            fluid: Fluid::None,
            brightness: 1.0,
            feet_y: 70.0,
            blindness: None,
            night_vision: 0.0,
        };
        let f = fog_color(noon, &input);
        // Between the provider fog color (0.7529, 0.8471, 1.0) and the sky color.
        assert!(f[0] > sky[0] && f[0] < 0.753);
        input.feet_y = 16.0;
        let low = fog_color(noon, &input);
        assert!((low[0] - f[0] * 0.25).abs() < 1e-6);
        input.fluid = Fluid::Water;
        input.feet_y = 70.0;
        assert_eq!(fog_color(noon, &input), [0.02, 0.02, 0.2]);
    }

    #[test]
    fn fog_brightness_follows_the_light() {
        let (prev, next) = fog_brightness_tick(0.0, 1.0, 16);
        assert_eq!(prev, 0.0);
        assert!((next - 0.1).abs() < 1e-6);
        assert_eq!(linear_fog(10, 0), (120.0, 160.0));
        assert_eq!(linear_fog(10, -1), (0.0, 160.0));
    }

    #[test]
    fn moon_phases_cycle_every_eight_days() {
        assert_eq!(moon_phase(0), 0);
        assert_eq!(moon_phase(24000 * 9), 1);
        assert_eq!(liquid_height_percent(0), 1.0 / 9.0);
        assert_eq!(liquid_height_percent(8), 1.0 / 9.0);
    }
}
