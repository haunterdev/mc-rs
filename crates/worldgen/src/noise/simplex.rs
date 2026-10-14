//! `net.minecraft.world.gen.NoiseGeneratorSimplex`: 2D simplex noise.

use crate::java_random::JavaRandom;

const GRAD3: [[i32; 3]; 12] = [
    [1, 1, 0],
    [-1, 1, 0],
    [1, -1, 0],
    [-1, -1, 0],
    [1, 0, 1],
    [-1, 0, 1],
    [1, 0, -1],
    [-1, 0, -1],
    [0, 1, 1],
    [0, -1, 1],
    [0, 1, -1],
    [0, -1, -1],
];

/// `NoiseGeneratorSimplex.SQRT_3`, `F2` and `G2`, computed the way the static initializer does.
#[inline(always)]
fn sqrt_3() -> f64 {
    3.0f64.sqrt()
}
#[inline(always)]
fn f2() -> f64 {
    0.5 * (sqrt_3() - 1.0)
}
#[inline(always)]
fn g2() -> f64 {
    (3.0 - sqrt_3()) / 6.0
}

/// `NoiseGeneratorSimplex`
#[derive(Clone)]
pub struct NoiseGeneratorSimplex {
    p: [i32; 512],
    pub xo: f64,
    pub yo: f64,
    pub zo: f64,
}

/// `NoiseGeneratorSimplex.fastFloor`: note `value > 0 ? (int)value : (int)value - 1`, so whole
/// non-positive numbers floor one too low. Ported as is.
#[inline(always)]
fn fast_floor(value: f64) -> i32 {
    if value > 0.0 {
        value as i32
    } else {
        value as i32 - 1
    }
}

/// `NoiseGeneratorSimplex.dot`
#[inline(always)]
fn dot(g: &[i32; 3], x: f64, y: f64) -> f64 {
    g[0] as f64 * x + g[1] as f64 * y
}

impl NoiseGeneratorSimplex {
    /// `NoiseGeneratorSimplex(Random)`
    pub fn new(rand: &mut JavaRandom) -> Self {
        let xo = rand.next_double() * 256.0;
        let yo = rand.next_double() * 256.0;
        let zo = rand.next_double() * 256.0;
        let mut p = [0i32; 512];
        for (i, v) in p.iter_mut().enumerate().take(256) {
            *v = i as i32;
        }
        for l in 0..256usize {
            let j = (rand.next_int_bounded(256 - l as i32) + l as i32) as usize;
            p.swap(l, j);
            p[l + 256] = p[l];
        }
        NoiseGeneratorSimplex { p, xo, yo, zo }
    }

    #[inline(always)]
    fn p(&self, i: i32) -> i32 {
        self.p[i as usize]
    }

    /// Shared corner math of `getValue` and `add`; both inline the same steps in vanilla.
    #[inline(always)]
    fn sample(&self, x: f64, y: f64) -> f64 {
        let d3 = f2();
        let d4 = (x + y) * d3;
        let i = fast_floor(x + d4);
        let j = fast_floor(y + d4);
        let d5 = g2();
        let d6 = i.wrapping_add(j) as f64 * d5;
        let d7 = i as f64 - d6;
        let d8 = j as f64 - d6;
        let d9 = x - d7;
        let d10 = y - d8;
        let (k, l) = if d9 > d10 { (1, 0) } else { (0, 1) };
        let d11 = d9 - k as f64 + d5;
        let d12 = d10 - l as f64 + d5;
        let d13 = d9 - 1.0 + 2.0 * d5;
        let d14 = d10 - 1.0 + 2.0 * d5;
        let i1 = i & 255;
        let j1 = j & 255;
        let k1 = (self.p(i1 + self.p(j1)) % 12) as usize;
        let l1 = (self.p(i1 + k + self.p(j1 + l)) % 12) as usize;
        let i2 = (self.p(i1 + 1 + self.p(j1 + 1)) % 12) as usize;
        let mut d15 = 0.5 - d9 * d9 - d10 * d10;
        let d0 = if d15 < 0.0 {
            0.0
        } else {
            d15 *= d15;
            d15 * d15 * dot(&GRAD3[k1], d9, d10)
        };
        let mut d16 = 0.5 - d11 * d11 - d12 * d12;
        let d1 = if d16 < 0.0 {
            0.0
        } else {
            d16 *= d16;
            d16 * d16 * dot(&GRAD3[l1], d11, d12)
        };
        let mut d17 = 0.5 - d13 * d13 - d14 * d14;
        let d2 = if d17 < 0.0 {
            0.0
        } else {
            d17 *= d17;
            d17 * d17 * dot(&GRAD3[i2], d13, d14)
        };
        70.0 * (d0 + d1 + d2)
    }

    /// `NoiseGeneratorSimplex.getValue(double, double)`. No `xo`/`yo` offset, unlike `add`.
    pub fn get_value(&self, x: f64, y: f64) -> f64 {
        self.sample(x, y)
    }

    /// `NoiseGeneratorSimplex.add`: accumulates `70 * (...) * amplitude` into `out`, row-major
    /// with the second coordinate outer.
    #[allow(clippy::too_many_arguments)]
    pub fn add(
        &self,
        out: &mut [f64],
        x_offset: f64,
        y_offset: f64,
        width: usize,
        height: usize,
        x_scale: f64,
        y_scale: f64,
        amplitude: f64,
    ) {
        let mut i = 0usize;
        for j in 0..height {
            let d0 = (y_offset + j as f64) * y_scale + self.yo;
            for k in 0..width {
                let d1 = (x_offset + k as f64) * x_scale + self.xo;
                out[i] += self.sample(d1, d0) * amplitude;
                i += 1;
            }
        }
    }
}
