//! `net.minecraft.world.gen.NoiseGeneratorOctaves`

use super::improved::NoiseGeneratorImproved;
use crate::java_random::JavaRandom;
use crate::math_helper;

/// `NoiseGeneratorOctaves`
#[derive(Clone)]
pub struct NoiseGeneratorOctaves {
    generators: Vec<NoiseGeneratorImproved>,
}

impl NoiseGeneratorOctaves {
    /// `NoiseGeneratorOctaves(Random, int)`: each octave consumes the shared Random in turn.
    pub fn new(rand: &mut JavaRandom, octaves: usize) -> Self {
        NoiseGeneratorOctaves { generators: (0..octaves).map(|_| NoiseGeneratorImproved::new(rand)).collect() }
    }

    /// `NoiseGeneratorOctaves.generateNoiseOctaves(double[], int, int, int, int, int, int, double, double, double)`.
    /// `out` is zeroed and resized to `x_size * y_size * z_size`.
    #[allow(clippy::too_many_arguments)]
    pub fn generate_noise_octaves(
        &self,
        out: &mut Vec<f64>,
        x_offset: i32,
        y_offset: i32,
        z_offset: i32,
        x_size: usize,
        y_size: usize,
        z_size: usize,
        x_scale: f64,
        y_scale: f64,
        z_scale: f64,
    ) {
        out.clear();
        out.resize(x_size * y_size * z_size, 0.0);
        let mut d3 = 1.0;
        for g in &self.generators {
            let mut d0 = x_offset as f64 * d3 * x_scale;
            let d1 = y_offset as f64 * d3 * y_scale;
            let mut d2 = z_offset as f64 * d3 * z_scale;
            let mut k = math_helper::lfloor(d0);
            let mut l = math_helper::lfloor(d2);
            d0 -= k as f64;
            d2 -= l as f64;
            k %= 16_777_216;
            l %= 16_777_216;
            d0 += k as f64;
            d2 += l as f64;
            g.populate_noise_array(
                out,
                d0,
                d1,
                d2,
                x_size,
                y_size,
                z_size,
                x_scale * d3,
                y_scale * d3,
                z_scale * d3,
                d3,
            );
            d3 /= 2.0;
        }
    }

    /// `NoiseGeneratorOctaves.generateNoiseOctaves(double[], int, int, int, int, double, double, double)`.
    /// The 2D overload samples at y offset 10 with y scale 1.0 and ignores its last argument.
    #[allow(clippy::too_many_arguments)]
    pub fn generate_noise_octaves_2d(
        &self,
        out: &mut Vec<f64>,
        x_offset: i32,
        z_offset: i32,
        x_size: usize,
        z_size: usize,
        x_scale: f64,
        z_scale: f64,
        _unused_exponent: f64,
    ) {
        self.generate_noise_octaves(out, x_offset, 10, z_offset, x_size, 1, z_size, x_scale, 1.0, z_scale);
    }
}
