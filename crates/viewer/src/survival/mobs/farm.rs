//! The pig, sheep and chicken models as data (`ModelPig`, `ModelSheep1`, `ModelSheep2`,
//! `ModelChicken`), with their pose functions. The table entries live in `models::MOBS`.
//!
//! Every `ModelQuadruped(height, scale)` is laid out here by hand from its constructor: head point
//! `(0, 18 - height, -6)`, body point `(0, 17 - height, 2)`, legs at `24 - height` and z 7 / -5.
//!
//! DROPPED: `ModelBase.setLivingAnimations` as a call (the sheep's head values arrive in the
//! `Frame`, see `anim.rs`), and the `jeb_` rainbow in `LayerSheepWool` (no custom names).

use bevy::prelude::*;
use worldgen::math_helper::cos;

use super::living::Frame;
use super::models::{quadruped_pose, BoxDef, PartDef};

const fn bx(u: i32, v: i32, min: [f32; 3], size: [i32; 3], grow: f32) -> BoxDef {
    BoxDef { uv: (u, v), min, size, grow }
}

/// `ModelPig(scale)`: `ModelQuadruped(6, scale)` plus the snout `head.setTextureOffset(16, 16)
/// .addBox(-2, 0, -9, 4, 3, 1, scale)`.
macro_rules! pig_parts {
    ($g:expr) => {
        &[
            PartDef { pivot: [0.0, 12.0, -6.0], boxes: &[bx(0, 0, [-4.0, -4.0, -8.0], [8, 8, 8], $g), bx(16, 16, [-2.0, 0.0, -9.0], [4, 3, 1], $g)] },
            PartDef { pivot: [0.0, 11.0, 2.0], boxes: &[bx(28, 8, [-5.0, -10.0, -7.0], [10, 16, 8], $g)] },
            PartDef { pivot: [-3.0, 18.0, 7.0], boxes: &[bx(0, 16, [-2.0, 0.0, -2.0], [4, 6, 4], $g)] },
            PartDef { pivot: [3.0, 18.0, 7.0], boxes: &[bx(0, 16, [-2.0, 0.0, -2.0], [4, 6, 4], $g)] },
            PartDef { pivot: [-3.0, 18.0, -5.0], boxes: &[bx(0, 16, [-2.0, 0.0, -2.0], [4, 6, 4], $g)] },
            PartDef { pivot: [3.0, 18.0, -5.0], boxes: &[bx(0, 16, [-2.0, 0.0, -2.0], [4, 6, 4], $g)] },
        ]
    };
}
/// `RenderPig`'s `new ModelPig()`
pub const PIG_PARTS: &[PartDef] = pig_parts!(0.0);
/// `LayerSaddle`'s `new ModelPig(0.5F)`
pub const PIG_SADDLE_PARTS: &[PartDef] = pig_parts!(0.5);

/// `ModelSheep2`, the sheared body `RenderSheep` draws: the head and body replaced, the legs those
/// of `ModelQuadruped(12, 0)`.
pub const SHEEP_PARTS: &[PartDef] = &[
    PartDef { pivot: [0.0, 6.0, -8.0], boxes: &[bx(0, 0, [-3.0, -4.0, -6.0], [6, 6, 8], 0.0)] },
    PartDef { pivot: [0.0, 5.0, 2.0], boxes: &[bx(28, 8, [-4.0, -10.0, -7.0], [8, 16, 6], 0.0)] },
    PartDef { pivot: [-3.0, 12.0, 7.0], boxes: &[bx(0, 16, [-2.0, 0.0, -2.0], [4, 12, 4], 0.0)] },
    PartDef { pivot: [3.0, 12.0, 7.0], boxes: &[bx(0, 16, [-2.0, 0.0, -2.0], [4, 12, 4], 0.0)] },
    PartDef { pivot: [-3.0, 12.0, -5.0], boxes: &[bx(0, 16, [-2.0, 0.0, -2.0], [4, 12, 4], 0.0)] },
    PartDef { pivot: [3.0, 12.0, -5.0], boxes: &[bx(0, 16, [-2.0, 0.0, -2.0], [4, 12, 4], 0.0)] },
];

/// `ModelSheep1`, the wool `LayerSheepWool` draws over it: the boxes grown by 0.6, 1.75 and 0.5.
pub const SHEEP_WOOL_PARTS: &[PartDef] = &[
    PartDef { pivot: [0.0, 6.0, -8.0], boxes: &[bx(0, 0, [-3.0, -4.0, -4.0], [6, 6, 6], 0.6)] },
    PartDef { pivot: [0.0, 5.0, 2.0], boxes: &[bx(28, 8, [-4.0, -10.0, -7.0], [8, 16, 6], 1.75)] },
    PartDef { pivot: [-3.0, 12.0, 7.0], boxes: &[bx(0, 16, [-2.0, 0.0, -2.0], [4, 6, 4], 0.5)] },
    PartDef { pivot: [3.0, 12.0, 7.0], boxes: &[bx(0, 16, [-2.0, 0.0, -2.0], [4, 6, 4], 0.5)] },
    PartDef { pivot: [-3.0, 12.0, -5.0], boxes: &[bx(0, 16, [-2.0, 0.0, -2.0], [4, 6, 4], 0.5)] },
    PartDef { pivot: [3.0, 12.0, -5.0], boxes: &[bx(0, 16, [-2.0, 0.0, -2.0], [4, 6, 4], 0.5)] },
];

/// `ModelChicken`: head, bill, chin, body, right leg, left leg, right wing, left wing.
pub const CHICKEN_PARTS: &[PartDef] = &[
    PartDef { pivot: [0.0, 15.0, -4.0], boxes: &[bx(0, 0, [-2.0, -6.0, -2.0], [4, 6, 3], 0.0)] },
    PartDef { pivot: [0.0, 15.0, -4.0], boxes: &[bx(14, 0, [-2.0, -4.0, -4.0], [4, 2, 2], 0.0)] },
    PartDef { pivot: [0.0, 15.0, -4.0], boxes: &[bx(14, 4, [-1.0, -2.0, -3.0], [2, 2, 2], 0.0)] },
    PartDef { pivot: [0.0, 16.0, 0.0], boxes: &[bx(0, 9, [-3.0, -4.0, -3.0], [6, 8, 6], 0.0)] },
    PartDef { pivot: [-2.0, 19.0, 1.0], boxes: &[bx(26, 0, [-1.0, 0.0, -3.0], [3, 5, 3], 0.0)] },
    PartDef { pivot: [1.0, 19.0, 1.0], boxes: &[bx(26, 0, [-1.0, 0.0, -3.0], [3, 5, 3], 0.0)] },
    PartDef { pivot: [-4.0, 13.0, 0.0], boxes: &[bx(24, 13, [0.0, 0.0, -3.0], [1, 4, 6], 0.0)] },
    PartDef { pivot: [4.0, 13.0, 0.0], boxes: &[bx(24, 13, [-1.0, 0.0, -3.0], [1, 4, 6], 0.0)] },
];

/// `ModelSheep1/2.setRotationAngles`: the quadruped's, then `head.rotateAngleX = headRotationAngleX`
/// (set by `setLivingAnimations`, carried in the frame).
pub fn sheep_pose(f: &Frame) -> Vec<Vec3> {
    let mut a = quadruped_pose(f);
    if let Some(x) = f.head_x {
        a[0].x = x;
    }
    a
}

/// `ModelChicken.setRotationAngles`. `f.age` is `handleRotationFloat`'s wing value here.
pub fn chicken_pose(f: &Frame) -> Vec<Vec3> {
    let rad = 0.017_453_292;
    let swing = f.limb_swing * 0.6662;
    let a = f.limb_amount;
    let head = Vec3::new(f.head_pitch * rad, f.net_head_yaw * rad, 0.0);
    vec![
        head,
        head,
        head,
        Vec3::new(1.570_796_4, 0.0, 0.0),
        Vec3::new(cos(swing) * 1.4 * a, 0.0, 0.0),
        Vec3::new(cos(swing + std::f32::consts::PI) * 1.4 * a, 0.0, 0.0),
        Vec3::new(0.0, 0.0, f.age),
        Vec3::new(0.0, 0.0, -f.age),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pivots(p: &[PartDef]) -> Vec<[f32; 3]> {
        p.iter().map(|p| p.pivot).collect()
    }

    fn frame() -> Frame {
        Frame { body_yaw: 0.0, net_head_yaw: 0.0, head_pitch: 0.0, age: 0.0, limb_swing: 0.0, limb_amount: 0.0, head_dy: 0.0, head_x: None, ..Default::default() }
    }

    #[test]
    fn the_pig_is_a_six_high_quadruped_with_a_snout() {
        assert_eq!(pivots(PIG_PARTS), [[0.0, 12.0, -6.0], [0.0, 11.0, 2.0], [-3.0, 18.0, 7.0], [3.0, 18.0, 7.0], [-3.0, 18.0, -5.0], [3.0, 18.0, -5.0]]);
        assert_eq!(PIG_PARTS[0].boxes.len(), 2);
        assert_eq!((PIG_PARTS[0].boxes[1].uv, PIG_PARTS[0].boxes[1].min, PIG_PARTS[0].boxes[1].size), ((16, 16), [-2.0, 0.0, -9.0], [4, 3, 1]));
        assert_eq!(PIG_PARTS[2].boxes[0].size, [4, 6, 4]);
    }

    #[test]
    fn the_saddle_is_the_pig_grown_by_half_a_unit() {
        assert_eq!(pivots(PIG_SADDLE_PARTS), pivots(PIG_PARTS));
        for (a, b) in PIG_PARTS.iter().zip(PIG_SADDLE_PARTS) {
            assert_eq!(a.boxes.len(), b.boxes.len());
            for (x, y) in a.boxes.iter().zip(b.boxes) {
                assert_eq!((x.uv, x.min, x.size, x.grow), (y.uv, y.min, y.size, 0.0));
                assert_eq!(y.grow, 0.5);
            }
        }
    }

    #[test]
    fn the_sheep_body_and_its_wool_share_points_and_the_wool_has_short_legs() {
        assert_eq!(pivots(SHEEP_PARTS), pivots(SHEEP_WOOL_PARTS));
        assert_eq!(pivots(SHEEP_PARTS)[0], [0.0, 6.0, -8.0]);
        assert_eq!(pivots(SHEEP_PARTS)[1], [0.0, 5.0, 2.0]);
        assert_eq!(SHEEP_PARTS[0].boxes[0].size, [6, 6, 8]);
        assert_eq!(SHEEP_PARTS[2].boxes[0].size, [4, 12, 4]);
        let grows: Vec<f32> = SHEEP_WOOL_PARTS.iter().map(|p| p.boxes[0].grow).collect();
        assert_eq!(grows, [0.6, 1.75, 0.5, 0.5, 0.5, 0.5]);
        assert_eq!(SHEEP_WOOL_PARTS[0].boxes[0].min, [-3.0, -4.0, -4.0]);
        assert_eq!(SHEEP_WOOL_PARTS[2].boxes[0].size, [4, 6, 4]);
    }

    #[test]
    fn the_chicken_has_eight_parts_with_the_head_three_sharing_a_point() {
        assert_eq!(CHICKEN_PARTS.len(), 8);
        assert!(CHICKEN_PARTS[..3].iter().all(|p| p.pivot == [0.0, 15.0, -4.0]));
        assert_eq!(pivots(CHICKEN_PARTS)[3], [0.0, 16.0, 0.0]);
        assert_eq!(pivots(CHICKEN_PARTS)[4..], [[-2.0, 19.0, 1.0], [1.0, 19.0, 1.0], [-4.0, 13.0, 0.0], [4.0, 13.0, 0.0]]);
        assert_eq!(CHICKEN_PARTS[6].boxes[0].size, [1, 4, 6]);
        assert_eq!(CHICKEN_PARTS[7].boxes[0].min, [-1.0, 0.0, -3.0]);
    }

    #[test]
    fn the_sheep_head_angle_replaces_the_quadruped_one() {
        let f = Frame { head_pitch: 30.0, ..frame() };
        assert!((sheep_pose(&f)[0].x - 30.0 * 0.017_453_292).abs() < 1e-6);
        let f = Frame { head_x: Some(0.628), ..f };
        assert_eq!(sheep_pose(&f)[0].x, 0.628);
        assert_eq!(sheep_pose(&f).len(), 6);
    }

    #[test]
    fn the_chicken_wings_flap_opposite_about_z_and_the_head_parts_move_together() {
        let f = Frame { age: 1.25, net_head_yaw: 40.0, head_pitch: -10.0, limb_swing: 1.0, limb_amount: 1.0, ..frame() };
        let a = chicken_pose(&f);
        assert_eq!((a[6].z, a[7].z), (1.25, -1.25));
        assert!(a[0] == a[1] && a[1] == a[2]);
        assert!((a[0].y - 40.0 * 0.017_453_292).abs() < 1e-6);
        assert_eq!(a[3].x, 1.570_796_4);
        assert!((a[4].x + a[5].x).abs() < 1e-3, "legs swing opposite");
    }
}
