//! The lightmap: `WorldProvider.generateLightBrightnessTable` and `EntityRenderer.updateLightmap`,
//! packed for the chunk shader. Vanilla uploads the 256 colors as a 16x16 texture sampled
//! `GL_LINEAR`; the shader does the same bilinear lookup from this table.

/// `WorldProvider.generateLightBrightnessTable` for the overworld (`f = 0.0F`).
pub fn brightness_table() -> [f32; 16] {
    std::array::from_fn(|i| {
        let f1 = 1.0 - i as f32 / 15.0;
        (1.0 - f1) / (f1 * 3.0 + 1.0) * 1.0 + 0.0
    })
}

/// `EntityRenderer.updateLightmap`: 256 ARGB colors, index `sky * 16 + block`.
/// `sun_brightness` is `World.getSunBrightness(1.0F)`, `gamma` is `GameSettings.gammaSetting`.
pub fn lightmap(sun_brightness: f32, gamma: f32, night_vision: f32) -> [u32; 256] {
    let table = brightness_table();
    let f = sun_brightness;
    let f1 = f * 0.95 + 0.05;
    std::array::from_fn(|i| {
        let f2 = table[i / 16] * f1;
        // DROPPED: torch flicker. `torchFlickerX` is a per-tick random walk around 0; held at 0.
        let f3 = table[i % 16] * (0.0 * 0.1 + 1.5);
        // DROPPED: the lightning flash branch (`getLastLightningBolt() > 0`); no weather.
        let f4 = f2 * (f * 0.65 + 0.35);
        let f5 = f2 * (f * 0.65 + 0.35);
        let f6 = f3 * ((f3 * 0.6 + 0.4) * 0.6 + 0.4);
        let f7 = f3 * (f3 * f3 * 0.6 + 0.4);
        let mut f8 = f4 + f3;
        let mut f9 = f5 + f6;
        let mut f10 = f2 + f7;
        f8 = f8 * 0.96 + 0.03;
        f9 = f9 * 0.96 + 0.03;
        f10 = f10 * 0.96 + 0.03;
        // Night vision lifts every channel toward full by the weakest one's headroom.
        if night_vision > 0.0 {
            let mut f18 = 1.0 / f8;
            if f18 > 1.0 / f9 {
                f18 = 1.0 / f9;
            }
            if f18 > 1.0 / f10 {
                f18 = 1.0 / f10;
            }
            f8 = f8 * (1.0 - night_vision) + f8 * f18 * night_vision;
            f9 = f9 * (1.0 - night_vision) + f9 * f18 * night_vision;
            f10 = f10 * (1.0 - night_vision) + f10 * f18 * night_vision;
        }
        // DROPPED: boss sky darkening and the End dimension branch.
        f8 = f8.min(1.0);
        f9 = f9.min(1.0);
        f10 = f10.min(1.0);
        let f16 = gamma;
        let mut f17 = 1.0 - f8;
        let mut f13 = 1.0 - f9;
        let mut f14 = 1.0 - f10;
        f17 = 1.0 - f17 * f17 * f17 * f17;
        f13 = 1.0 - f13 * f13 * f13 * f13;
        f14 = 1.0 - f14 * f14 * f14 * f14;
        f8 = f8 * (1.0 - f16) + f17 * f16;
        f9 = f9 * (1.0 - f16) + f13 * f16;
        f10 = f10 * (1.0 - f16) + f14 * f16;
        f8 = f8 * 0.96 + 0.03;
        f9 = f9 * 0.96 + 0.03;
        f10 = f10 * 0.96 + 0.03;
        f8 = f8.clamp(0.0, 1.0);
        f9 = f9.clamp(0.0, 1.0);
        f10 = f10.clamp(0.0, 1.0);
        let k = (f8 * 255.0) as u32;
        let l = (f9 * 255.0) as u32;
        let i1 = (f10 * 255.0) as u32;
        0xFF00_0000 | k << 16 | l << 8 | i1
    })
}

/// `GL_LINEAR` lookup at lightmap coordinates `(block, sky)` in 0..=240, as the fixed-function
/// path samples it (texture matrix `(coord + 8) / 256`). Returns RGB in 0..=1.
#[cfg(test)]
pub fn sample(colors: &[u32; 256], block: f32, sky: f32) -> [f32; 3] {
    let (u, v) = (block / 16.0, sky / 16.0);
    let (u0, v0) = (u.floor().clamp(0.0, 15.0), v.floor().clamp(0.0, 15.0));
    let (fu, fv) = (u - u0, v - v0);
    let (u0, v0) = (u0 as usize, v0 as usize);
    let (u1, v1) = ((u0 + 1).min(15), (v0 + 1).min(15));
    let c = |s: usize, b: usize, shift: u32| ((colors[s * 16 + b] >> shift) & 255) as f32 / 255.0;
    std::array::from_fn(|ch| {
        let shift = 16 - 8 * ch as u32;
        let top = c(v0, u0, shift) * (1.0 - fu) + c(v0, u1, shift) * fu;
        let bottom = c(v1, u0, shift) * (1.0 - fu) + c(v1, u1, shift) * fu;
        top * (1.0 - fv) + bottom * fv
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn brightness_table_ends() {
        let t = brightness_table();
        assert_eq!(t[0], 0.0);
        assert_eq!(t[15], 1.0);
        // 1 - 1/15 = 0.93333, (0.0667) / (3.8) = 0.017544
        assert!((t[1] - 0.017_543_86).abs() < 1e-6);
    }

    #[test]
    fn full_daylight_is_white_and_darkness_is_not_black() {
        let m = lightmap(1.0, 0.0, 0.0);
        // Sky 15, block 0 at full sun: f2 = 1, f4 = 1 -> 0.99, then 0.99 * 0.96 + 0.03 = 0.9804 -> 250.
        assert_eq!(m[15 * 16], 0xFF00_0000 | 250 << 16 | 250 << 8 | 250);
        // Sky 0, block 0: 0.03 * 0.96 + 0.03 = 0.0588 -> 14.
        assert_eq!(m[0], 0xFF00_0000 | 14 << 16 | 14 << 8 | 14);
        // Mid torch light is warm, red ahead of blue (block 10: f3 = 0.5, blue f7 = 0.275). Near
        // full block light both channels clamp to 1, so block 14 is white.
        let torch = m[10];
        assert!((torch >> 16 & 255) > (torch & 255));
    }

    #[test]
    fn gamma_brightens_the_dark() {
        let moody = lightmap(1.0, 0.0, 0.0);
        let bright = lightmap(1.0, 1.0, 0.0);
        assert!((bright[5 * 16] & 255) > (moody[5 * 16] & 255));
    }

    #[test]
    fn bilinear_lookup_hits_texel_centers() {
        let m = lightmap(1.0, 0.0, 0.0);
        let s = sample(&m, 0.0, 240.0);
        assert!((s[0] - 250.0 / 255.0).abs() < 1e-6);
        let half = sample(&m, 0.0, 232.0);
        let lo = (m[14 * 16] >> 16 & 255) as f32 / 255.0;
        assert!((half[0] - (lo + 250.0 / 255.0) / 2.0).abs() < 1e-6);
    }

    #[test]
    fn night_vision_brightens_a_dark_cell() {
        let off = lightmap(0.0, 0.0, 0.0);
        let on = lightmap(0.0, 0.0, 1.0);
        let dark = 0;
        let lum = |c: u32| (c >> 16 & 255) + (c >> 8 & 255) + (c & 255);
        assert!(lum(on[dark]) > lum(off[dark]), "{:x} vs {:x}", on[dark], off[dark]);
        assert_eq!(lightmap(0.0, 0.0, 0.0), off);
    }
}
