//! `MathHelper` pieces the path code and the helpers read: `atan2` (the table version, not
//! `Math.atan2`) and `wrapDegrees`.

use std::sync::OnceLock;

use sim::collision::fast_inv_sqrt;

/// `MathHelper.FRAC_BIAS`
const FRAC_BIAS: f64 = f64::from_bits(4805340802404319232);

/// `MathHelper.ASINE_TAB` and `COS_TAB`.
fn tables() -> &'static ([f64; 257], [f64; 257]) {
    static T: OnceLock<([f64; 257], [f64; 257])> = OnceLock::new();
    T.get_or_init(|| {
        let mut asine = [0.0; 257];
        let mut cos = [0.0; 257];
        for j in 0..257 {
            let d1 = (j as f64 / 256.0).asin();
            cos[j] = d1.cos();
            asine[j] = d1;
        }
        (asine, cos)
    })
}

/// `MathHelper.atan2(double, double)`
pub fn atan2(mut y: f64, mut x: f64) -> f64 {
    let d0 = x * x + y * y;
    if d0.is_nan() {
        return f64::NAN;
    }
    let flag = y < 0.0;
    if flag {
        y = -y;
    }
    let flag1 = x < 0.0;
    if flag1 {
        x = -x;
    }
    let flag2 = y > x;
    if flag2 {
        std::mem::swap(&mut x, &mut y);
    }
    let d9 = fast_inv_sqrt(d0);
    x *= d9;
    y *= d9;
    let d2 = FRAC_BIAS + y;
    let i = d2.to_bits() as i32 as usize;
    let (asine, cos) = tables();
    let d3 = asine[i];
    let d4 = cos[i];
    let d5 = d2 - FRAC_BIAS;
    let d6 = y * d4 - x * d5;
    let d7 = (6.0 + d6 * d6) * d6 * 0.166_666_666_666_666_66;
    let mut d8 = d3 + d7;
    if flag2 {
        d8 = std::f64::consts::FRAC_PI_2 - d8;
    }
    if flag1 {
        d8 = std::f64::consts::PI - d8;
    }
    if flag {
        d8 = -d8;
    }
    d8
}

/// `MathHelper.wrapDegrees(float)`
pub fn wrap_degrees(v: f32) -> f32 {
    let mut v = v % 360.0;
    if v >= 180.0 {
        v -= 360.0;
    }
    if v < -180.0 {
        v += 360.0;
    }
    v
}

/// `MathHelper.sqrt(float)`
pub fn sqrt_f(v: f32) -> f32 {
    (v as f64).sqrt() as f32
}
