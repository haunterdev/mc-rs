//! Bit-exact `java.util.Random` (JDK 8): the 48-bit LCG and every method worldgen calls.

use crate::fdlibm;

const MULTIPLIER: i64 = 0x5_DEEC_E66D;
const ADDEND: i64 = 0xB;
const MASK: i64 = (1 << 48) - 1;
/// `0x1.0p-53`
const DOUBLE_UNIT: f64 = 1.0 / (1u64 << 53) as f64;

/// `java.util.Random`.
#[derive(Clone, Debug)]
pub struct JavaRandom {
    seed: i64,
    next_next_gaussian: f64,
    have_next_next_gaussian: bool,
}

impl JavaRandom {
    /// `java.util.Random(long)`
    pub fn new(seed: i64) -> Self {
        let mut r = JavaRandom { seed: 0, next_next_gaussian: 0.0, have_next_next_gaussian: false };
        r.set_seed(seed);
        r
    }

    /// `java.util.Random.setSeed`: scrambles the seed and clears the cached gaussian.
    pub fn set_seed(&mut self, seed: i64) {
        self.seed = (seed ^ MULTIPLIER) & MASK;
        self.have_next_next_gaussian = false;
    }

    /// `java.util.Random.next(int)`
    #[inline]
    pub fn next(&mut self, bits: u32) -> i32 {
        self.seed = (self.seed.wrapping_mul(MULTIPLIER).wrapping_add(ADDEND)) & MASK;
        // (int)(seed >>> (48 - bits)): the seed is a non-negative 48-bit value.
        ((self.seed as u64) >> (48 - bits)) as i32
    }

    /// `java.util.Random.nextInt()`
    #[inline]
    pub fn next_int(&mut self) -> i32 {
        self.next(32)
    }

    /// `java.util.Random.nextInt(int)`, including the rejection loop for non-power-of-two bounds.
    /// Panics on `bound <= 0` where Java throws `IllegalArgumentException`.
    #[inline]
    pub fn next_int_bounded(&mut self, bound: i32) -> i32 {
        assert!(bound > 0, "bound must be positive");
        if (bound & bound.wrapping_neg()) == bound {
            return ((bound as i64).wrapping_mul(self.next(31) as i64) >> 31) as i32;
        }
        loop {
            let bits = self.next(31);
            let val = bits % bound;
            if bits.wrapping_sub(val).wrapping_add(bound - 1) >= 0 {
                return val;
            }
        }
    }

    /// `java.util.Random.nextLong()`
    #[inline]
    pub fn next_long(&mut self) -> i64 {
        ((self.next(32) as i64) << 32).wrapping_add(self.next(32) as i64)
    }

    /// `java.util.Random.nextBoolean()`
    #[inline]
    pub fn next_boolean(&mut self) -> bool {
        self.next(1) != 0
    }

    /// `java.util.Random.nextFloat()`
    #[inline]
    pub fn next_float(&mut self) -> f32 {
        self.next(24) as f32 / (1 << 24) as f32
    }

    /// `java.util.Random.nextDouble()`
    #[inline]
    pub fn next_double(&mut self) -> f64 {
        (((self.next(26) as i64) << 27) + self.next(27) as i64) as f64 * DOUBLE_UNIT
    }

    /// `java.util.Random.nextGaussian()`: polar method, `StrictMath.log` and `StrictMath.sqrt`.
    pub fn next_gaussian(&mut self) -> f64 {
        if self.have_next_next_gaussian {
            self.have_next_next_gaussian = false;
            return self.next_next_gaussian;
        }
        let (mut v1, mut v2, mut s);
        loop {
            v1 = 2.0 * self.next_double() - 1.0;
            v2 = 2.0 * self.next_double() - 1.0;
            s = v1 * v1 + v2 * v2;
            if !(s >= 1.0 || s == 0.0) {
                break;
            }
        }
        let multiplier = (-2.0 * fdlibm::log(s) / s).sqrt();
        self.next_next_gaussian = v2 * multiplier;
        self.have_next_next_gaussian = true;
        v1 * multiplier
    }
}
