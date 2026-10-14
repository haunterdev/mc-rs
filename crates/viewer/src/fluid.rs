//! Water and lava: a port of `BlockFluidRenderer.renderFluid` and the `BlockLiquid` helpers it
//! calls, plus `MathHelper.atan2` for the flow direction.

use std::sync::OnceLock;

use worldgen::math_helper;

use crate::colors::Colors;
use crate::mesher::{ChunkMesh, FLAG_EMISSIVE};
use crate::models::{Facing, Models};
use crate::region::{props, Mat, Region};

/// `0.001F` widened to double, as the side quads use it.
const EPS: f32 = 0.0010000000474974513;

/// `BlockFluidRenderer.renderFluid`, chunk-relative.
#[allow(clippy::too_many_arguments)]
pub fn render(r: &Region, models: &Models, colors: &Colors, state: u16, x: i32, y: i32, z: i32, out: &mut ChunkMesh) {
    let p = props(state);
    let material = p.mat;
    let lava = material == Mat::Lava;
    let (still, flow) = if lava { (models.lava_still, models.lava_flow) } else { (models.water_still, models.water_flow) };
    let i = colors.multiplier(state, r, x, y, z);
    let f = (i >> 16 & 255) as f32 / 255.0;
    let f1 = (i >> 8 & 255) as f32 / 255.0;
    let f2 = (i & 255) as f32 / 255.0;
    let flag1 = side_rendered(r, material, x, y, z, Facing::Up);
    let flag2 = side_rendered(r, material, x, y, z, Facing::Down);
    let sides = [Facing::North, Facing::South, Facing::West, Facing::East].map(|s| side_rendered(r, material, x, y, z, s));
    if !flag1 && !flag2 && !sides.iter().any(|&s| s) {
        return;
    }
    let tag = if p.light > 0 { FLAG_EMISSIVE } else { 0 };
    let mesh = out.layer(p.layer);
    let mut f7 = fluid_height(r, x, y, z, material);
    let mut f8 = fluid_height(r, x, y, z + 1, material);
    let mut f9 = fluid_height(r, x + 1, y, z + 1, material);
    let mut f10 = fluid_height(r, x + 1, y, z, material);
    let (d0, d1, d2) = (x as f32, y as f32, z as f32);
    let byte = |v: f32| (v * 255.0) as i32 as u8;

    if flag1 {
        let f12 = slope_angle(r, x, y, z, state, material);
        let sprite = if f12 > -999.0 { flow } else { still };
        f7 -= 0.001;
        f8 -= 0.001;
        f9 -= 0.001;
        f10 -= 0.001;
        let uv: [[f32; 2]; 4] = if f12 < -999.0 {
            [[0.0, 0.0], [0.0, 1.0], [1.0, 1.0], [1.0, 0.0]]
        } else {
            let f21 = math_helper::sin(f12) * 0.25;
            let f22 = math_helper::cos(f12) * 0.25;
            let c = |v: f32| (8.0 + v * 16.0) / 16.0;
            [
                [c(-f22 - f21), c(-f22 + f21)],
                [c(-f22 + f21), c(f22 + f21)],
                [c(f22 + f21), c(f22 - f21)],
                [c(f22 - f21), c(-f22 - f21)],
            ]
        };
        let k2 = liquid_light(r, x, y, z);
        let color = [byte(1.0 * f), byte(1.0 * f1), byte(1.0 * f2)];
        let pos = [[d0, d1 + f7, d2], [d0, d1 + f8, d2 + 1.0], [d0 + 1.0, d1 + f9, d2 + 1.0], [d0 + 1.0, d1 + f10, d2]];
        mesh.quad(pos, uv, [k2; 4], [color; 4], [0.0, 1.0, 0.0], sprite.layer | tag);
        if should_render_sides(r, material, x, y + 1, z) {
            let back = [0, 3, 2, 1];
            mesh.quad(back.map(|k| pos[k]), back.map(|k| uv[k]), [k2; 4], [color; 4], [0.0, -1.0, 0.0], sprite.layer | tag);
        }
    }

    if flag2 {
        let l1 = liquid_light(r, x, y - 1, z);
        let pos = [[d0, d1, d2 + 1.0], [d0, d1, d2], [d0 + 1.0, d1, d2], [d0 + 1.0, d1, d2 + 1.0]];
        let uv = [[0.0, 1.0], [0.0, 0.0], [1.0, 0.0], [1.0, 1.0]];
        mesh.quad(pos, uv, [l1; 4], [[byte(0.5); 3]; 4], [0.0, -1.0, 0.0], still.layer | tag);
    }

    for (i1, face) in [Facing::North, Facing::South, Facing::West, Facing::East].into_iter().enumerate() {
        // DROPPED: the water_overlay sprite for a side facing glass; the generator places no glass,
        // so every side takes the flow sprite and the back quad.
        if !sides[i1] {
            continue;
        }
        let o = face.vec();
        let (f39, f40, d3, d5, d4, d6) = match i1 {
            0 => (f7, f10, d0, d0 + 1.0, d2 + EPS, d2 + EPS),
            1 => (f9, f8, d0 + 1.0, d0, d2 + 1.0 - EPS, d2 + 1.0 - EPS),
            2 => (f8, f7, d0 + EPS, d0 + EPS, d2 + 1.0, d2),
            _ => (f10, f9, d0 + 1.0 - EPS, d0 + 1.0 - EPS, d2, d2 + 1.0),
        };
        let f41 = 0.0;
        let f27 = 0.5;
        let f28 = (1.0 - f39) * 16.0 * 0.5 / 16.0;
        let f29 = (1.0 - f40) * 16.0 * 0.5 / 16.0;
        let f30 = 0.5;
        let j = liquid_light(r, x + o[0], y, z + o[2]);
        let f31 = if i1 < 2 { 0.8f32 } else { 0.6 };
        let color = [byte(1.0 * f31 * f), byte(1.0 * f31 * f1), byte(1.0 * f31 * f2)];
        let n = [o[0] as f32, 0.0, o[2] as f32];
        mesh.quad(
            [[d3, d1 + f39, d4], [d5, d1 + f40, d6], [d5, d1, d6], [d3, d1, d4]],
            [[f41, f28], [f27, f29], [f27, f30], [f41, f30]],
            [j; 4],
            [color; 4],
            n,
            flow.layer | tag,
        );
        mesh.quad(
            [[d3, d1, d4], [d5, d1, d6], [d5, d1 + f40, d6], [d3, d1 + f39, d4]],
            [[f41, f30], [f27, f30], [f27, f29], [f41, f28]],
            [j; 4],
            [color; 4],
            n.map(|v| -v),
            flow.layer | tag,
        );
    }
}

/// `BlockLiquid.shouldSideBeRendered`: never toward the same material, always upward, otherwise
/// `Block.shouldSideBeRendered` with the full-block bounding box.
fn side_rendered(r: &Region, material: Mat, x: i32, y: i32, z: i32, f: Facing) -> bool {
    let o = f.vec();
    let n = r.props(x + o[0], y + o[1], z + o[2]);
    if n.mat == material {
        false
    } else if f == Facing::Up {
        true
    } else {
        !n.opaque_cube
    }
}

/// `BlockLiquid.getLiquidHeightPercent`
pub fn liquid_height_percent(mut meta: i32) -> f32 {
    if meta >= 8 {
        meta = 0;
    }
    (meta + 1) as f32 / 9.0
}

/// `BlockFluidRenderer.getFluidHeight`: the surface height at the corner shared by the block and
/// its neighbors at -x, -z and -x-z.
pub fn fluid_height(r: &Region, x: i32, y: i32, z: i32, material: Mat) -> f32 {
    let mut i = 0i32;
    let mut f = 0.0f32;
    for j in 0..4 {
        let (bx, bz) = (x - (j & 1), z - (j >> 1 & 1));
        if r.props(bx, y + 1, bz).mat == material {
            return 1.0;
        }
        let s = r.state(bx, y, bz);
        let p = props(s);
        if p.mat != material {
            if !p.solid_material() {
                f += 1.0;
                i += 1;
            }
        } else {
            let k = (s & 15) as i32;
            if k >= 8 || k == 0 {
                f += liquid_height_percent(k) * 10.0;
                i += 10;
            }
            f += liquid_height_percent(k);
            i += 1;
        }
    }
    1.0 - f / i as f32
}

/// `BlockLiquid.getRenderedDepth`
fn rendered_depth(state: u16, material: Mat) -> i32 {
    if props(state).mat == material {
        let level = (state & 15) as i32;
        if level >= 8 {
            0
        } else {
            level
        }
    } else {
        -1
    }
}

/// `BlockLiquid.getFlow`, normalized as `Vec3d.normalize` does.
fn flow(r: &Region, x: i32, y: i32, z: i32, state: u16, material: Mat) -> [f64; 3] {
    let mut d = [0.0f64; 3];
    let i = rendered_depth(state, material);
    for f in Facing::HORIZONTAL {
        let o = f.vec();
        let (nx, nz) = (x + o[0], z + o[2]);
        let ns = r.state(nx, y, nz);
        let mut j = rendered_depth(ns, material);
        if j < 0 {
            if !props(ns).blocks_movement() {
                j = rendered_depth(r.state(nx, y - 1, nz), material);
                if j >= 0 {
                    let k = j - (i - 8);
                    for a in 0..3 {
                        d[a] += (o[a] * k) as f64;
                    }
                }
            }
        } else if j >= 0 {
            let l = j - i;
            for a in 0..3 {
                d[a] += (o[a] * l) as f64;
            }
        }
    }
    // DROPPED: the LEVEL >= 8 downward current (causesDownwardCurrent); the generator places only
    // level 0 fluids.
    // Vec3d.normalize: MathHelper.sqrt(double) is a float square root.
    let d0 = ((d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt() as f32) as f64;
    if d0 < 1.0E-4 {
        [0.0; 3]
    } else {
        [d[0] / d0, d[1] / d0, d[2] / d0]
    }
}

/// `BlockLiquid.getSlopeAngle`: -1000 for still fluid.
fn slope_angle(r: &Region, x: i32, y: i32, z: i32, state: u16, material: Mat) -> f32 {
    let v = flow(r, x, y, z, state, material);
    if v[0] == 0.0 && v[2] == 0.0 {
        -1000.0
    } else {
        atan2(v[2], v[0]) as f32 - 1.5707964
    }
}

/// `BlockLiquid.getPackedLightmapCoords`: the brighter of the block and the one above, per
/// component, without the block's own light value.
fn liquid_light(r: &Region, x: i32, y: i32, z: i32) -> i32 {
    let i = r.combined_light(x, y, z, 0);
    let j = r.combined_light(x, y + 1, z, 0);
    let (k, l) = (i & 255, j & 255);
    let (i1, j1) = (i >> 16 & 255, j >> 16 & 255);
    k.max(l) | i1.max(j1) << 16
}

/// `BlockLiquid.shouldRenderSides`: some block in the 3x3 at `y` is neither this fluid nor a full
/// block.
fn should_render_sides(r: &Region, material: Mat, x: i32, y: i32, z: i32) -> bool {
    for i in -1..=1 {
        for j in -1..=1 {
            let p = r.props(x + i, y, z + j);
            // isFullBlock: the `fullBlock` field, the default state's isOpaqueCube.
            if p.mat != material && !p.opaque_cube {
                return true;
            }
        }
    }
    false
}

/// `MathHelper.ASINE_TAB` and `COS_TAB`. Java fills them with `Math.asin` / `Math.cos`; the
/// platform's may differ in the last bit, below what survives the cast to float.
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

/// `MathHelper.fastInvSqrt`
fn fast_inv_sqrt(v: f64) -> f64 {
    let d0 = 0.5 * v;
    let i = 6910469410427058090i64.wrapping_sub((v.to_bits() as i64) >> 1);
    let v = f64::from_bits(i as u64);
    v * (1.5 - d0 * v * v)
}

/// `MathHelper.atan2`: a table lookup plus a cubic correction, not `Math.atan2`.
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
    // FRAC_BIAS = Double.longBitsToDouble(4805340802404319232L)
    let frac_bias = f64::from_bits(4805340802404319232);
    let d2 = frac_bias + y;
    let i = d2.to_bits() as i32 as usize;
    let (asine, cos) = tables();
    let d3 = asine[i];
    let d4 = cos[i];
    let d5 = d2 - frac_bias;
    let d6 = y * d4 - x * d5;
    let d7 = (6.0 + d6 * d6) * d6 * 0.16666666666666666;
    let mut d8 = d3 + d7;
    if flag2 {
        d8 = 1.5707963267948966 - d8;
    }
    if flag1 {
        d8 = 3.141592653589793 - d8;
    }
    if flag {
        d8 = -d8;
    }
    d8
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::region::tests::{chunk, region_of};
    use worldgen::blocks::WATER;

    #[test]
    fn still_water_stands_eight_ninths_high() {
        let r = region_of(
            chunk(0, 0, 60, |p| {
                for x in 0..16 {
                    for z in 0..16 {
                        p.set(x, 62, z, WATER);
                        p.set(x, 63, z, WATER);
                        for y in 60..62 {
                            p.set(x, y, z, worldgen::blocks::STONE);
                        }
                    }
                }
            }),
            60,
        );
        assert_eq!(fluid_height(&r, 5, 63, 5, Mat::Water), 1.0 - (44.0f32 / 9.0) / 44.0);
        // Water under water reads full height.
        assert_eq!(fluid_height(&r, 5, 62, 5, Mat::Water), 1.0);
        assert_eq!(slope_angle(&r, 5, 63, 5, WATER, Mat::Water), -1000.0);
    }

    #[test]
    fn fast_atan2_tracks_the_real_one() {
        for (y, x) in [(1.0, 0.0), (0.0, 1.0), (-1.0, -1.0), (0.3, -0.7), (-0.9, 0.1)] {
            let (a, b) = (atan2(y, x), f64::atan2(y, x));
            assert!((a - b).abs() < 1e-6, "atan2({y}, {x}) = {a}, want {b}");
        }
    }
}
