//! `net.minecraft.world.gen.NoiseGeneratorPerlin`: layered simplex noise.

use super::simplex::NoiseGeneratorSimplex;
use crate::java_random::JavaRandom;

/// `NoiseGeneratorPerlin`
#[derive(Clone)]
pub struct NoiseGeneratorPerlin {
    levels: Vec<NoiseGeneratorSimplex>,
}

impl NoiseGeneratorPerlin {
    /// `NoiseGeneratorPerlin(Random, int)`
    pub fn new(rand: &mut JavaRandom, levels: usize) -> Self {
        NoiseGeneratorPerlin { levels: (0..levels).map(|_| NoiseGeneratorSimplex::new(rand)).collect() }
    }

    /// `NoiseGeneratorPerlin.getValue(double, double)`
    pub fn get_value(&self, x: f64, z: f64) -> f64 {
        let mut d0 = 0.0;
        let mut d1 = 1.0;
        for level in &self.levels {
            d0 += level.get_value(x * d1, z * d1) / d1;
            d1 /= 2.0;
        }
        d0
    }

    /// `NoiseGeneratorPerlin.getRegion(double[], double, double, int, int, double, double, double)`,
    /// the overload that passes 0.5 as the last factor.
    #[allow(clippy::too_many_arguments)]
    pub fn get_region(
        &self,
        out: &mut Vec<f64>,
        x: f64,
        z: f64,
        x_size: usize,
        z_size: usize,
        x_scale: f64,
        z_scale: f64,
        p12: f64,
    ) {
        self.get_region_with(out, x, z, x_size, z_size, x_scale, z_scale, p12, 0.5);
    }

    /// `NoiseGeneratorPerlin.getRegion(double[], double, double, int, int, double, double, double, double)`
    #[allow(clippy::too_many_arguments)]
    pub fn get_region_with(
        &self,
        out: &mut Vec<f64>,
        x: f64,
        z: f64,
        x_size: usize,
        z_size: usize,
        x_scale: f64,
        z_scale: f64,
        p12: f64,
        p14: f64,
    ) {
        out.clear();
        out.resize(x_size * z_size, 0.0);
        let mut d1 = 1.0;
        let mut d0 = 1.0;
        for level in &self.levels {
            level.add(out, x, z, x_size, z_size, x_scale * d0 * d1, z_scale * d0 * d1, 0.55 / d1);
            d0 *= p12;
            d1 *= p14;
        }
    }
}
