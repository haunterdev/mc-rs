//! `net.minecraft.util.math.MathHelper` and the `java.lang.Math` calls worldgen makes.

use std::sync::LazyLock;

/// `MathHelper.SIN_TABLE`: `(float)Math.sin(i * PI * 2 / 65536)` for i in 0..65536.
static SIN_TABLE: LazyLock<Box<[f32]>> = LazyLock::new(|| {
    (0..65536)
        .map(|i| (i as f64 * 3.141592653589793 * 2.0 / 65536.0).sin() as f32)
        .collect()
});

/// The 65536-entry table, exposed for the fixture comparison.
pub fn sin_table() -> &'static [f32] {
    &SIN_TABLE
}

/// `MathHelper.sin(float)`
#[inline]
pub fn sin(value: f32) -> f32 {
    SIN_TABLE[((value * 10430.378_f32) as i32 & 0xffff) as usize]
}

/// `MathHelper.cos(float)`
#[inline]
pub fn cos(value: f32) -> f32 {
    SIN_TABLE[((value * 10430.378_f32 + 16384.0_f32) as i32 & 0xffff) as usize]
}

/// `MathHelper.sqrt(float)`: `(float)Math.sqrt((double)value)`.
#[inline]
pub fn sqrt_f(value: f32) -> f32 {
    (value as f64).sqrt() as f32
}

/// `MathHelper.floor(double)`: saturating cast, then step down for negatives. The step wraps, so
/// a value below `i32::MIN` gives `i32::MAX` as in Java.
#[inline]
pub fn floor(value: f64) -> i32 {
    let i = value as i32;
    if value < i as f64 {
        i.wrapping_sub(1)
    } else {
        i
    }
}

/// `MathHelper.floor(float)`
#[inline]
pub fn floor_f(value: f32) -> i32 {
    let i = value as i32;
    if value < i as f32 {
        i.wrapping_sub(1)
    } else {
        i
    }
}

/// `MathHelper.ceil(float)`
#[inline]
pub fn ceil_f(value: f32) -> i32 {
    let i = value as i32;
    if value > i as f32 {
        i.wrapping_add(1)
    } else {
        i
    }
}

/// `MathHelper.lfloor(double)`
#[inline]
pub fn lfloor(value: f64) -> i64 {
    let i = value as i64;
    if value < i as f64 {
        i.wrapping_sub(1)
    } else {
        i
    }
}

/// `MathHelper.clampedLerp(double, double, double)`
#[inline]
pub fn clamped_lerp(lower: f64, upper: f64, slide: f64) -> f64 {
    if slide < 0.0 {
        lower
    } else if slide > 1.0 {
        upper
    } else {
        lower + (upper - lower) * slide
    }
}

/// `java.lang.Math.round(double)` as current JDK 8 updates implement it (the JDK-8010430 fix):
/// round half up on the exact significand. The older `(long)floor(a + 0.5)` gives a different
/// answer only for odd integers at or above 2^52, which `BiomeMesa.getBand` never reaches.
#[inline]
pub fn java_round(a: f64) -> i64 {
    let bits = a.to_bits() as i64;
    let biased_exp = (bits & 0x7ff0_0000_0000_0000) >> 52;
    let shift = (53 - 2 + 1023) - biased_exp;
    if shift & -64 == 0 {
        let mut r = (bits & 0x000f_ffff_ffff_ffff) | 0x0010_0000_0000_0000;
        if bits < 0 {
            r = -r;
        }
        ((r >> shift) + 1) >> 1
    } else {
        a as i64
    }
}
