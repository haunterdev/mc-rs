//! `net.minecraft.world.gen.NoiseGeneratorImproved`: one octave of improved Perlin noise.

use crate::java_random::JavaRandom;

const GRAD_X: [f64; 16] = [1.0, -1.0, 1.0, -1.0, 1.0, -1.0, 1.0, -1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, -1.0, 0.0];
const GRAD_Y: [f64; 16] = [1.0, 1.0, -1.0, -1.0, 0.0, 0.0, 0.0, 0.0, 1.0, -1.0, 1.0, -1.0, 1.0, -1.0, 1.0, -1.0];
const GRAD_Z: [f64; 16] = [0.0, 0.0, 0.0, 0.0, 1.0, 1.0, -1.0, -1.0, 1.0, 1.0, -1.0, -1.0, 0.0, 1.0, 0.0, -1.0];
const GRAD_2X: [f64; 16] = [1.0, -1.0, 1.0, -1.0, 1.0, -1.0, 1.0, -1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, -1.0, 0.0];
const GRAD_2Z: [f64; 16] = [0.0, 0.0, 0.0, 0.0, 1.0, 1.0, -1.0, -1.0, 1.0, 1.0, -1.0, -1.0, 0.0, 1.0, 0.0, -1.0];

/// `NoiseGeneratorImproved`
#[derive(Clone)]
pub struct NoiseGeneratorImproved {
    permutations: [i32; 512],
    pub x_coord: f64,
    pub y_coord: f64,
    pub z_coord: f64,
}

/// `NoiseGeneratorImproved.lerp`
#[inline(always)]
fn lerp(t: f64, a: f64, b: f64) -> f64 {
    a + t * (b - a)
}

/// `NoiseGeneratorImproved.grad2`
#[inline(always)]
fn grad2(hash: i32, x: f64, z: f64) -> f64 {
    let i = (hash & 15) as usize;
    GRAD_2X[i] * x + GRAD_2Z[i] * z
}

/// `NoiseGeneratorImproved.grad`
#[inline(always)]
fn grad(hash: i32, x: f64, y: f64, z: f64) -> f64 {
    let i = (hash & 15) as usize;
    GRAD_X[i] * x + GRAD_Y[i] * y + GRAD_Z[i] * z
}

/// `(int)d` then step down when `d` was negative and fractional.
#[inline(always)]
fn floor_i32(d: f64) -> i32 {
    let i = d as i32;
    if d < i as f64 {
        i - 1
    } else {
        i
    }
}

#[inline(always)]
fn fade(t: f64) -> f64 {
    t * t * t * (t * (t * 6.0 - 15.0) + 10.0)
}

impl NoiseGeneratorImproved {
    /// `NoiseGeneratorImproved(Random)`: three offsets, then a Fisher-Yates shuffle.
    pub fn new(rand: &mut JavaRandom) -> Self {
        let x_coord = rand.next_double() * 256.0;
        let y_coord = rand.next_double() * 256.0;
        let z_coord = rand.next_double() * 256.0;
        let mut p = [0i32; 512];
        for (i, v) in p.iter_mut().enumerate().take(256) {
            *v = i as i32;
        }
        for l in 0..256usize {
            let j = (rand.next_int_bounded(256 - l as i32) + l as i32) as usize;
            p.swap(l, j);
            p[l + 256] = p[l];
        }
        NoiseGeneratorImproved { permutations: p, x_coord, y_coord, z_coord }
    }

    #[inline(always)]
    fn p(&self, i: i32) -> i32 {
        self.permutations[i as usize]
    }

    /// `NoiseGeneratorImproved.populateNoiseArray`. With `y_size == 1` this takes the 2D path,
    /// which ignores `y_offset` and mixes `grad2` with `grad(.., 0.0, ..)` exactly as vanilla does.
    #[allow(clippy::too_many_arguments)]
    pub fn populate_noise_array(
        &self,
        noise: &mut [f64],
        x_offset: f64,
        y_offset: f64,
        z_offset: f64,
        x_size: usize,
        y_size: usize,
        z_size: usize,
        x_scale: f64,
        y_scale: f64,
        z_scale: f64,
        noise_scale: f64,
    ) {
        if y_size == 1 {
            let mut l5 = 0usize;
            let d16 = 1.0 / noise_scale;
            for j2 in 0..x_size {
                let mut d17 = x_offset + j2 as f64 * x_scale + self.x_coord;
                let i6 = floor_i32(d17);
                let k2 = i6 & 255;
                d17 -= i6 as f64;
                let d18 = fade(d17);
                for j6 in 0..z_size {
                    let mut d19 = z_offset + j6 as f64 * z_scale + self.z_coord;
                    let k6 = floor_i32(d19);
                    let l6 = k6 & 255;
                    d19 -= k6 as f64;
                    let d20 = fade(d19);
                    let i5 = self.p(k2);
                    let j5 = self.p(i5) + l6;
                    let j = self.p(k2 + 1);
                    let k5 = self.p(j) + l6;
                    let d14 = lerp(d18, grad2(self.p(j5), d17, d19), grad(self.p(k5), d17 - 1.0, 0.0, d19));
                    let d15 = lerp(
                        d18,
                        grad(self.p(j5 + 1), d17, 0.0, d19 - 1.0),
                        grad(self.p(k5 + 1), d17 - 1.0, 0.0, d19 - 1.0),
                    );
                    let d21 = lerp(d20, d14, d15);
                    noise[l5] += d21 * d16;
                    l5 += 1;
                }
            }
        } else {
            let mut i = 0usize;
            let d0 = 1.0 / noise_scale;
            // The cache key persists across columns within one call, as in vanilla.
            let mut k: i32 = -1;
            let (mut d1, mut d2, mut d3, mut d4) = (0.0, 0.0, 0.0, 0.0);
            for l2 in 0..x_size {
                let mut d5 = x_offset + l2 as f64 * x_scale + self.x_coord;
                let i3 = floor_i32(d5);
                let j3 = i3 & 255;
                d5 -= i3 as f64;
                let d6 = fade(d5);
                for k3 in 0..z_size {
                    let mut d7 = z_offset + k3 as f64 * z_scale + self.z_coord;
                    let l3 = floor_i32(d7);
                    let i4 = l3 & 255;
                    d7 -= l3 as f64;
                    let d8 = fade(d7);
                    for j4 in 0..y_size {
                        let mut d9 = y_offset + j4 as f64 * y_scale + self.y_coord;
                        let k4 = floor_i32(d9);
                        let l4 = k4 & 255;
                        d9 -= k4 as f64;
                        let d10 = fade(d9);
                        if j4 == 0 || l4 != k {
                            k = l4;
                            let l = self.p(j3) + l4;
                            let i1 = self.p(l) + i4;
                            let j1 = self.p(l + 1) + i4;
                            let k1 = self.p(j3 + 1) + l4;
                            let l1 = self.p(k1) + i4;
                            let i2 = self.p(k1 + 1) + i4;
                            d1 = lerp(d6, grad(self.p(i1), d5, d9, d7), grad(self.p(l1), d5 - 1.0, d9, d7));
                            d2 = lerp(
                                d6,
                                grad(self.p(j1), d5, d9 - 1.0, d7),
                                grad(self.p(i2), d5 - 1.0, d9 - 1.0, d7),
                            );
                            d3 = lerp(
                                d6,
                                grad(self.p(i1 + 1), d5, d9, d7 - 1.0),
                                grad(self.p(l1 + 1), d5 - 1.0, d9, d7 - 1.0),
                            );
                            d4 = lerp(
                                d6,
                                grad(self.p(j1 + 1), d5, d9 - 1.0, d7 - 1.0),
                                grad(self.p(i2 + 1), d5 - 1.0, d9 - 1.0, d7 - 1.0),
                            );
                        }
                        let d11 = lerp(d10, d1, d2);
                        let d12 = lerp(d10, d3, d4);
                        let d13 = lerp(d8, d11, d12);
                        noise[i] += d13 * d0;
                        i += 1;
                    }
                }
            }
        }
    }
}
