//! The zombie, skeleton, creeper and spider as data (`ModelZombie`, `ModelSkeleton`,
//! `ModelCreeper`, `ModelSpider`) with their pose functions, plus the creeper's swell
//! (`RenderCreeper.preRenderCallback`, `getColorMultiplier`). The table entries are the consts
//! below, listed in `models::MOBS`.
//!
//! `ModelBiped`'s part order is the order `render` draws them: head, body, right arm, left arm,
//! right leg, left leg, then the headwear (which a child draws inside the 0.5 group with the body).
//! The left arm and leg are `mirror` renderers (`MobDef::mirrored`). Arm points and the legs'
//! z of 0.1 are what `setRotationAngles` leaves them at, so they are the `PartDef` points here;
//! only the swing moves the arms (`pivot_shift`).
//!
//! DROPPED: `LayerBipedArmor`, `LayerCustomHead` and `LayerElytra` (the viewer has no mob armor
//! slots drawn), the sneaking, riding and elytra branches of `ModelBiped.setRotationAngles`
//! (no mob does any), the left-handed mirror of the bow and swing branches (every mob is right
//! handed), `ModelSkeleton`'s `p_i46303_2_` armor variant, and the shadow blobs.

use std::f32::consts::PI;

use bevy::prelude::*;
use worldgen::math_helper::{cos, sin, sqrt_f};

use super::living::Frame;
use super::models::{BoxDef, ChildLayout, Hand, LayerDef, LayerKind, MobDef, MobSounds, PartDef};

const RAD: f32 = 0.017_453_292;
const HALF_PI: f32 = 1.570_796_4;

const fn bx(u: i32, v: i32, min: [f32; 3], size: [i32; 3], grow: f32) -> BoxDef {
    BoxDef { uv: (u, v), min, size, grow }
}

const fn part(pivot: [f32; 3], b: &'static [BoxDef]) -> PartDef {
    PartDef { pivot, boxes: b }
}

/// `ModelBiped(0, 0, 64, _)`: head, body, right arm, left arm, right leg, left leg, headwear.
pub const BIPED_PARTS: &[PartDef] = &[
    part([0.0, 0.0, 0.0], &[bx(0, 0, [-4.0, -8.0, -4.0], [8, 8, 8], 0.0)]),
    part([0.0, 0.0, 0.0], &[bx(16, 16, [-4.0, 0.0, -2.0], [8, 12, 4], 0.0)]),
    part([-5.0, 2.0, 0.0], &[bx(40, 16, [-3.0, -2.0, -2.0], [4, 12, 4], 0.0)]),
    part([5.0, 2.0, 0.0], &[bx(40, 16, [-1.0, -2.0, -2.0], [4, 12, 4], 0.0)]),
    part([-1.9, 12.0, 0.1], &[bx(0, 16, [-2.0, 0.0, -2.0], [4, 12, 4], 0.0)]),
    part([1.9, 12.0, 0.1], &[bx(0, 16, [-2.0, 0.0, -2.0], [4, 12, 4], 0.0)]),
    part([0.0, 0.0, 0.0], &[bx(32, 0, [-4.0, -8.0, -4.0], [8, 8, 8], 0.5)]),
];

/// `ModelSkeleton`: the biped with 2 x 12 x 2 arms and legs, the legs at x of 2.
pub const SKELETON_PARTS: &[PartDef] = &[
    part([0.0, 0.0, 0.0], &[bx(0, 0, [-4.0, -8.0, -4.0], [8, 8, 8], 0.0)]),
    part([0.0, 0.0, 0.0], &[bx(16, 16, [-4.0, 0.0, -2.0], [8, 12, 4], 0.0)]),
    part([-5.0, 2.0, 0.0], &[bx(40, 16, [-1.0, -2.0, -1.0], [2, 12, 2], 0.0)]),
    part([5.0, 2.0, 0.0], &[bx(40, 16, [-1.0, -2.0, -1.0], [2, 12, 2], 0.0)]),
    part([-2.0, 12.0, 0.1], &[bx(0, 16, [-1.0, 0.0, -1.0], [2, 12, 2], 0.0)]),
    part([2.0, 12.0, 0.1], &[bx(0, 16, [-1.0, 0.0, -1.0], [2, 12, 2], 0.0)]),
    part([0.0, 0.0, 0.0], &[bx(32, 0, [-4.0, -8.0, -4.0], [8, 8, 8], 0.5)]),
];

const CREEPER_LEG: &[BoxDef] = &[bx(0, 16, [-2.0, 0.0, -2.0], [4, 6, 4], 0.0)];
/// `ModelCreeper(0)`: head, body, leg1 to leg4 (`creeperArmor` is never rendered).
pub const CREEPER_PARTS: &[PartDef] = &[
    part([0.0, 6.0, 0.0], &[bx(0, 0, [-4.0, -8.0, -4.0], [8, 8, 8], 0.0)]),
    part([0.0, 6.0, 0.0], &[bx(16, 16, [-4.0, 0.0, -2.0], [8, 12, 4], 0.0)]),
    part([-2.0, 18.0, 4.0], CREEPER_LEG),
    part([2.0, 18.0, 4.0], CREEPER_LEG),
    part([-2.0, 18.0, -4.0], CREEPER_LEG),
    part([2.0, 18.0, -4.0], CREEPER_LEG),
];

const CHARGE_LEG: &[BoxDef] = &[bx(0, 16, [-2.0, 0.0, -2.0], [4, 6, 4], 2.0)];
/// `LayerCreeperCharge`'s `ModelCreeper(2.0F)`.
pub const CREEPER_CHARGE_PARTS: &[PartDef] = &[
    part([0.0, 6.0, 0.0], &[bx(0, 0, [-4.0, -8.0, -4.0], [8, 8, 8], 2.0)]),
    part([0.0, 6.0, 0.0], &[bx(16, 16, [-4.0, 0.0, -2.0], [8, 12, 4], 2.0)]),
    part([-2.0, 18.0, 4.0], CHARGE_LEG),
    part([2.0, 18.0, 4.0], CHARGE_LEG),
    part([-2.0, 18.0, -4.0], CHARGE_LEG),
    part([2.0, 18.0, -4.0], CHARGE_LEG),
];

const SPIDER_LEFT: &[BoxDef] = &[bx(18, 0, [-15.0, -1.0, -1.0], [16, 2, 2], 0.0)];
const SPIDER_RIGHT: &[BoxDef] = &[bx(18, 0, [-1.0, -1.0, -1.0], [16, 2, 2], 0.0)];
/// `ModelSpider`: head, neck, body, then legs 1 to 8 (odd ones on the -x side).
pub const SPIDER_PARTS: &[PartDef] = &[
    part([0.0, 15.0, -3.0], &[bx(32, 4, [-4.0, -4.0, -8.0], [8, 8, 8], 0.0)]),
    part([0.0, 15.0, 0.0], &[bx(0, 0, [-3.0, -3.0, -3.0], [6, 6, 6], 0.0)]),
    part([0.0, 15.0, 9.0], &[bx(0, 12, [-5.0, -4.0, -6.0], [10, 8, 12], 0.0)]),
    part([-4.0, 15.0, 2.0], SPIDER_LEFT),
    part([4.0, 15.0, 2.0], SPIDER_RIGHT),
    part([-4.0, 15.0, 1.0], SPIDER_LEFT),
    part([4.0, 15.0, 1.0], SPIDER_RIGHT),
    part([-4.0, 15.0, 0.0], SPIDER_LEFT),
    part([4.0, 15.0, 0.0], SPIDER_RIGHT),
    part([-4.0, 15.0, -1.0], SPIDER_LEFT),
    part([4.0, 15.0, -1.0], SPIDER_RIGHT),
];

/// `ModelBiped.setRotationAngles` as far as a mob reaches it: the rotation of each part and how
/// far the swing moved the arms' points from `PartDef::pivot`. `bow` is `rightArmPose ==
/// BOW_AND_ARROW`.
struct Biped {
    rot: [Vec3; 7],
    shift: [Vec3; 7],
}

fn biped(f: &Frame, bow: bool) -> Biped {
    let mut rot = [Vec3::ZERO; 7];
    let mut shift = [Vec3::ZERO; 7];
    let (ls, la) = (f.limb_swing, f.limb_amount);
    // head: bipedHead.rotateAngleX/Y
    rot[0] = Vec3::new(f.head_pitch * RAD, f.net_head_yaw * RAD, 0.0);
    // arms (the divisor f is 1 without an elytra) and legs
    rot[2].x = cos(ls * 0.6662 + PI) * 2.0 * la * 0.5;
    rot[3].x = cos(ls * 0.6662) * 2.0 * la * 0.5;
    rot[4].x = cos(ls * 0.6662) * 1.4 * la;
    rot[5].x = cos(ls * 0.6662 + PI) * 1.4 * la;
    // EMPTY arm poses: both arms' y is 0
    if f.swing > 0.0 {
        // the main hand is the right one: the right arm swings
        let by = sin(sqrt_f(f.swing) * 6.283_185_5) * 0.2;
        rot[1].y = by;
        shift[2].z = sin(by) * 5.0;
        shift[2].x = -cos(by) * 5.0 + 5.0;
        shift[3].z = -sin(by) * 5.0;
        shift[3].x = cos(by) * 5.0 - 5.0;
        rot[2].y += by;
        rot[3].y += by;
        rot[3].x += by;
        let mut f1 = 1.0 - f.swing;
        f1 *= f1;
        f1 *= f1;
        f1 = 1.0 - f1;
        let f2 = sin(f1 * PI);
        let f3 = sin(f.swing * PI) * -(rot[0].x - 0.7) * 0.75;
        rot[2].x = (rot[2].x as f64 - (f2 as f64 * 1.2 + f3 as f64)) as f32;
        rot[2].y += by * 2.0;
        rot[2].z += sin(f.swing * PI) * -0.4;
    }
    rot[2].z += cos(f.age * 0.09) * 0.05 + 0.05;
    rot[3].z -= cos(f.age * 0.09) * 0.05 + 0.05;
    rot[2].x += sin(f.age * 0.067) * 0.05;
    rot[3].x -= sin(f.age * 0.067) * 0.05;
    if bow {
        rot[2].y = -0.1 + rot[0].y;
        rot[3].y = 0.1 + rot[0].y + 0.4;
        rot[2].x = -HALF_PI + rot[0].x;
        rot[3].x = -HALF_PI + rot[0].x;
    }
    // copyModelAngles(bipedHead, bipedHeadwear)
    rot[6] = rot[0];
    Biped { rot, shift }
}

/// `ModelZombie.setRotationAngles`
fn zombie(f: &Frame) -> Biped {
    let mut b = biped(f, false);
    let raised = f.flags & proto::MM_AGGRESSIVE != 0;
    let s = sin(f.swing * PI);
    let s1 = sin((1.0 - (1.0 - f.swing) * (1.0 - f.swing)) * PI);
    b.rot[2].z = 0.0;
    b.rot[3].z = 0.0;
    b.rot[2].y = -(0.1 - s * 0.6);
    b.rot[3].y = 0.1 - s * 0.6;
    let x = -PI / if raised { 1.5 } else { 2.25 };
    b.rot[2].x = x + (s * 1.2 - s1 * 0.4);
    b.rot[3].x = x + (s * 1.2 - s1 * 0.4);
    b.rot[2].z += cos(f.age * 0.09) * 0.05 + 0.05;
    b.rot[3].z -= cos(f.age * 0.09) * 0.05 + 0.05;
    b.rot[2].x += sin(f.age * 0.067) * 0.05;
    b.rot[3].x -= sin(f.age * 0.067) * 0.05;
    b
}

pub fn zombie_pose(f: &Frame) -> Vec<Vec3> {
    zombie(f).rot.to_vec()
}

pub fn zombie_shift(f: &Frame) -> Vec<Vec3> {
    zombie(f).shift.to_vec()
}

/// `ModelSkeleton.setLivingAnimations` and `setRotationAngles`: a bow in the main hand while
/// `isSwingingArms` is the bow-aim pose; swinging arms without a bow are raised like a zombie's.
fn skeleton(f: &Frame) -> Biped {
    let swinging = f.flags & proto::MM_AGGRESSIVE != 0;
    let mut b = biped(f, swinging && f.bow);
    if swinging && !f.bow {
        let s = sin(f.swing * PI);
        let s1 = sin((1.0 - (1.0 - f.swing) * (1.0 - f.swing)) * PI);
        b.rot[2].z = 0.0;
        b.rot[3].z = 0.0;
        b.rot[2].y = -(0.1 - s * 0.6);
        b.rot[3].y = 0.1 - s * 0.6;
        b.rot[2].x = -HALF_PI - (s * 1.2 - s1 * 0.4);
        b.rot[3].x = -HALF_PI - (s * 1.2 - s1 * 0.4);
        b.rot[2].z += cos(f.age * 0.09) * 0.05 + 0.05;
        b.rot[3].z -= cos(f.age * 0.09) * 0.05 + 0.05;
        b.rot[2].x += sin(f.age * 0.067) * 0.05;
        b.rot[3].x -= sin(f.age * 0.067) * 0.05;
    }
    b
}

pub fn skeleton_pose(f: &Frame) -> Vec<Vec3> {
    skeleton(f).rot.to_vec()
}

pub fn skeleton_shift(f: &Frame) -> Vec<Vec3> {
    skeleton(f).shift.to_vec()
}

/// `ModelCreeper.setRotationAngles`
pub fn creeper_pose(f: &Frame) -> Vec<Vec3> {
    let (ls, la) = (f.limb_swing * 0.6662, f.limb_amount);
    vec![
        Vec3::new(f.head_pitch * RAD, f.net_head_yaw * RAD, 0.0),
        Vec3::ZERO,
        Vec3::new(cos(ls) * 1.4 * la, 0.0, 0.0),
        Vec3::new(cos(ls + PI) * 1.4 * la, 0.0, 0.0),
        Vec3::new(cos(ls + PI) * 1.4 * la, 0.0, 0.0),
        Vec3::new(cos(ls) * 1.4 * la, 0.0, 0.0),
    ]
}

/// `RenderCreeper.preRenderCallback`'s `scale(f2, f3, f2)` for a flash intensity.
pub fn creeper_scale(flash: f32) -> Vec3 {
    let f1 = 1.0 + sin(flash * 100.0) * flash * 0.01;
    let f = flash.clamp(0.0, 1.0);
    let f = f * f;
    let f = f * f;
    Vec3::new((1.0 + f * 0.4) * f1, (1.0 + f * 0.1) / f1, (1.0 + f * 0.4) * f1)
}

/// `RenderCreeper.getColorMultiplier`'s alpha as a 0 to 1 strength of the white flash. The alpha
/// byte is `(int)(f * 0.2 * 255)` clamped, OR 0x30: vanilla writes `i << 24 | 822083583`, whose
/// top byte is 0x30.
pub fn creeper_flash(flash: f32) -> f32 {
    if (flash * 10.0) as i32 % 2 == 0 {
        return 0.0;
    }
    let i = ((flash * 0.2 * 255.0) as i32).clamp(0, 255) as u32;
    ((i | 0x30) & 255) as f32 / 255.0
}

/// `ModelSpider.setRotationAngles`: head, neck and body at rest, then the eight legs.
pub fn spider_pose(f: &Frame) -> Vec<Vec3> {
    let (ls, la) = (f.limb_swing, f.limb_amount);
    let mut z = [-0.785_398_2, 0.785_398_2, -0.581_194_64, 0.581_194_64, -0.581_194_64, 0.581_194_64, -0.785_398_2, 0.785_398_2];
    let mut y = [0.785_398_2, -0.785_398_2, 0.392_699_1, -0.392_699_1, -0.392_699_1, 0.392_699_1, -0.785_398_2, 0.785_398_2];
    let w = ls * 0.6662;
    let f3 = -(cos(w * 2.0) * 0.4) * la;
    let f4 = -(cos(w * 2.0 + PI) * 0.4) * la;
    let f5 = -(cos(w * 2.0 + HALF_PI) * 0.4) * la;
    let f6 = -(cos(w * 2.0 + 4.712_389) * 0.4) * la;
    let f7 = (sin(w) * 0.4).abs() * la;
    let f8 = (sin(w + PI) * 0.4).abs() * la;
    let f9 = (sin(w + HALF_PI) * 0.4).abs() * la;
    let f10 = (sin(w + 4.712_389) * 0.4).abs() * la;
    for (i, (dy, dz)) in [(f3, f7), (-f3, -f7), (f4, f8), (-f4, -f8), (f5, f9), (-f5, -f9), (f6, f10), (-f6, -f10)].into_iter().enumerate() {
        y[i] += dy;
        z[i] += dz;
    }
    let mut out = vec![Vec3::new(f.head_pitch * RAD, f.net_head_yaw * RAD, 0.0), Vec3::ZERO, Vec3::ZERO];
    out.extend((0..8).map(|i| Vec3::new(0.0, y[i], z[i])));
    out
}

const BIPED_CHILD: ChildLayout = ChildLayout { head_offset: [16.0, 0.0], body_drop: 24.0, head_scale: 0.75 };

pub const ZOMBIE: MobDef = MobDef {
    kind: proto::MOB_ZOMBIE,
    name: "zombie",
    // RenderZombie.ZOMBIE_TEXTURES
    texture: "textures/entity/zombie/zombie.png",
    // ModelZombie(0, false): 64 x 64
    tex_size: (64.0, 64.0),
    parts: BIPED_PARTS,
    heads: &[0],
    child: BIPED_CHILD,
    pose: zombie_pose,
    mirrored: &[3, 5],
    shift: Some(zombie_shift),
    death_max: 90.0,
    // RenderBiped adds LayerHeldItem; ModelBiped.postRenderArm is the plain arm render
    hand: Some(Hand { part: 2, dx: 0.0 }),
    // EntityZombie.getEyeHeight: 1.74, minus 0.81 for a baby
    eye_height: (1.74, 1.74 - 0.81),
    // EntityZombie.setSize(0.6, 1.95)
    size: (0.6, 1.95),
    sounds: MobSounds { hurt: "entity.zombie.hurt", death: "entity.zombie.death", volume: 1.0 },
    layers: &[],
};

pub const SKELETON: MobDef = MobDef {
    kind: proto::MOB_SKELETON,
    name: "skeleton",
    // RenderSkeleton.SKELETON_TEXTURES
    texture: "textures/entity/skeleton/skeleton.png",
    tex_size: (64.0, 32.0),
    parts: SKELETON_PARTS,
    heads: &[0],
    child: BIPED_CHILD,
    pose: skeleton_pose,
    mirrored: &[3, 5],
    shift: Some(skeleton_shift),
    death_max: 90.0,
    // ModelSkeleton.postRenderArm moves the right arm's point 1 out
    hand: Some(Hand { part: 2, dx: 1.0 }),
    // AbstractSkeleton.getEyeHeight
    eye_height: (1.74, 1.74),
    // AbstractSkeleton.setSize(0.6, 1.99)
    size: (0.6, 1.99),
    sounds: MobSounds { hurt: "entity.skeleton.hurt", death: "entity.skeleton.death", volume: 1.0 },
    layers: &[],
};

pub const CREEPER: MobDef = MobDef {
    kind: proto::MOB_CREEPER,
    name: "creeper",
    // RenderCreeper.CREEPER_TEXTURES
    texture: "textures/entity/creeper/creeper.png",
    tex_size: (64.0, 32.0),
    parts: CREEPER_PARTS,
    heads: &[0],
    child: BIPED_CHILD,
    pose: creeper_pose,
    mirrored: &[],
    shift: None,
    death_max: 90.0,
    hand: None,
    // Entity.getEyeHeight is height * 0.85
    eye_height: (1.7 * 0.85, 1.7 * 0.85),
    // EntityCreeper.setSize(0.6, 1.7)
    size: (0.6, 1.7),
    sounds: MobSounds { hurt: "entity.creeper.hurt", death: "entity.creeper.death", volume: 1.0 },
    layers: &[LayerDef { kind: LayerKind::CreeperCharge, texture: "textures/entity/creeper/creeper_armor.png", parts: CREEPER_CHARGE_PARTS }],
};

pub const SPIDER: MobDef = MobDef {
    kind: proto::MOB_SPIDER,
    name: "spider",
    // RenderSpider.SPIDER_TEXTURES
    texture: "textures/entity/spider/spider.png",
    tex_size: (64.0, 32.0),
    parts: SPIDER_PARTS,
    heads: &[0],
    child: BIPED_CHILD,
    pose: spider_pose,
    mirrored: &[],
    shift: None,
    // RenderSpider.getDeathMaxRotation
    death_max: 180.0,
    hand: None,
    // EntitySpider.getEyeHeight
    eye_height: (0.65, 0.65),
    // EntitySpider.setSize(1.4, 0.9)
    size: (1.4, 0.9),
    sounds: MobSounds { hurt: "entity.spider.hurt", death: "entity.spider.death", volume: 1.0 },
    // LayerSpiderEyes renders the main model again with the eyes texture
    layers: &[LayerDef { kind: LayerKind::SpiderEyes, texture: "textures/entity/spider_eyes.png", parts: SPIDER_PARTS }],
};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::survival::mobs::models::def_of;

    fn frame() -> Frame {
        Frame::default()
    }

    #[test]
    fn the_part_counts_and_points_match_the_constructors() {
        assert_eq!((BIPED_PARTS.len(), SKELETON_PARTS.len(), CREEPER_PARTS.len(), CREEPER_CHARGE_PARTS.len(), SPIDER_PARTS.len()), (7, 7, 6, 6, 11));
        assert_eq!(BIPED_PARTS[2].pivot, [-5.0, 2.0, 0.0]);
        assert_eq!(BIPED_PARTS[4].pivot, [-1.9, 12.0, 0.1]);
        assert_eq!(SKELETON_PARTS[2].boxes[0].size, [2, 12, 2]);
        assert_eq!(SKELETON_PARTS[5].pivot, [2.0, 12.0, 0.1]);
        assert_eq!(CREEPER_PARTS[2].pivot, [-2.0, 18.0, 4.0]);
        assert!(CREEPER_CHARGE_PARTS.iter().all(|p| p.boxes[0].grow == 2.0));
        assert_eq!(BIPED_PARTS[6].boxes[0].grow, 0.5);
        assert_eq!(SPIDER_PARTS[2].pivot, [0.0, 15.0, 9.0]);
        assert_eq!(SPIDER_PARTS[3].boxes[0].min, [-15.0, -1.0, -1.0]);
        assert_eq!(SPIDER_PARTS[4].boxes[0].min, [-1.0, -1.0, -1.0]);
    }

    #[test]
    fn a_zombie_holds_its_arms_out_and_higher_when_aggressive() {
        let idle = zombie_pose(&frame());
        assert!((idle[2].x - (-PI / 2.25)).abs() < 1e-5 && (idle[3].x - (-PI / 2.25)).abs() < 1e-5);
        assert!((idle[2].y + 0.1).abs() < 1e-6 && (idle[3].y - 0.1).abs() < 1e-6);
        let up = zombie_pose(&Frame { flags: proto::MM_AGGRESSIVE, ..frame() });
        assert!((up[2].x - (-PI / 1.5)).abs() < 1e-5);
        assert!((up[2].z - 0.1).abs() < 1e-6 && (up[3].z + 0.1).abs() < 1e-6, "age 0: cos 0 * 0.05 + 0.05 on the right, minus it on the left");
    }

    #[test]
    fn the_attack_swing_lifts_the_zombie_arms_by_sine_and_moves_no_points_for_zombies_at_rest() {
        let rest = zombie_shift(&frame());
        assert!(rest.iter().all(|v| *v == Vec3::ZERO));
        let f = Frame { swing: 0.5, ..frame() };
        let a = zombie_pose(&f);
        let s = sin(0.5 * PI);
        let s1 = sin((1.0 - 0.25) * PI);
        assert!((a[2].x - (-PI / 2.25 + s * 1.2 - s1 * 0.4)).abs() < 1e-5);
        assert!((a[2].y - (-(0.1 - s * 0.6))).abs() < 1e-6);
        // the body turns and the arms' points follow it (ModelBiped's swingProgress block)
        let by = sin(sqrt_f(0.5) * 6.283_185_5) * 0.2;
        assert!((a[1].y - by).abs() < 1e-6);
        let shift = zombie_shift(&f);
        assert!((shift[2].z - sin(by) * 5.0).abs() < 1e-6 && (shift[3].z + sin(by) * 5.0).abs() < 1e-6);
        assert!((shift[2].x - (-cos(by) * 5.0 + 5.0)).abs() < 1e-6);
    }

    #[test]
    fn the_legs_and_head_follow_the_limb_swing_and_the_head_turn() {
        let f = Frame { limb_swing: 1.0, limb_amount: 1.0, net_head_yaw: 30.0, head_pitch: -20.0, ..frame() };
        let a = zombie_pose(&f);
        assert!((a[4].x - cos(0.6662) * 1.4).abs() < 1e-6 && (a[5].x - cos(0.6662 + PI) * 1.4).abs() < 1e-6);
        assert!((a[0].y - 30.0 * RAD).abs() < 1e-6 && (a[6].y - a[0].y).abs() < 1e-9 && a[6].x == a[0].x, "the headwear copies the head");
    }

    #[test]
    fn a_skeleton_aims_the_bow_only_when_it_holds_one_and_swings_its_arms() {
        let idle = skeleton_pose(&Frame { bow: true, ..frame() });
        assert!(idle[2].y.abs() < 1e-9 && idle[2].x.abs() < 0.06, "arms hang: {:?}", idle[2]);
        let aim = skeleton_pose(&Frame { bow: true, flags: proto::MM_AGGRESSIVE, head_pitch: 10.0, net_head_yaw: 20.0, ..frame() });
        let (hx, hy) = (10.0 * RAD, 20.0 * RAD);
        assert!((aim[2].x - (-HALF_PI + hx)).abs() < 1e-6 && (aim[3].x - (-HALF_PI + hx)).abs() < 1e-6);
        assert!((aim[2].y - (-0.1 + hy)).abs() < 1e-6 && (aim[3].y - (0.1 + hy + 0.4)).abs() < 1e-6);
        let raised = skeleton_pose(&Frame { flags: proto::MM_AGGRESSIVE, ..frame() });
        assert!((raised[2].x + HALF_PI).abs() < 1e-6, "no bow: both arms straight out");
    }

    #[test]
    fn the_left_limbs_are_the_mirrored_renderers() {
        for d in [ZOMBIE, SKELETON] {
            assert_eq!(d.mirrored, &[3, 5]);
        }
        assert_eq!(def_of(proto::MOB_SKELETON).unwrap().1.hand.as_ref().map(|h| (h.part, h.dx)), Some((2, 1.0)));
    }

    #[test]
    fn a_baby_biped_scales_the_head_by_three_quarters_and_the_rest_by_half() {
        let (_, z) = def_of(proto::MOB_ZOMBIE).unwrap();
        let adult = crate::survival::mobs::models::part_transforms(z, &frame(), false);
        let baby = crate::survival::mobs::models::part_transforms(z, &frame(), true);
        assert_eq!(baby[0].scale, Vec3::splat(0.75));
        assert_eq!(baby[0].translation, (adult[0].translation + Vec3::new(0.0, 16.0, 0.0)) * 0.75);
        for i in [1, 2, 3, 4, 5, 6] {
            assert_eq!(baby[i].scale, Vec3::splat(0.5), "part {i}");
            assert_eq!(baby[i].translation, (adult[i].translation + Vec3::new(0.0, 24.0, 0.0)) * 0.5);
        }
    }

    #[test]
    fn the_creeper_swells_by_the_fourth_power_of_its_flash() {
        assert_eq!(creeper_scale(0.0), Vec3::ONE);
        // flash 1: f1 = 1 + sin(100) * 0.01, f = 1
        let s = creeper_scale(1.0);
        let f1 = 1.0 + sin(100.0) * 0.01;
        assert!((s.x - 1.4 * f1).abs() < 1e-5 && (s.y - 1.1 / f1).abs() < 1e-5 && s.x == s.z);
        // flash 0.5: 0.5^4 = 0.0625 of the 0.4 and 0.1
        let s = creeper_scale(0.5);
        let f1 = 1.0 + sin(50.0) * 0.5 * 0.01;
        assert!((s.x - (1.0 + 0.0625 * 0.4) * f1).abs() < 1e-5 && (s.y - (1.0 + 0.0625 * 0.1) / f1).abs() < 1e-5);
        // past 1 the power term stops growing but the wobble keeps its factor
        assert!((creeper_scale(30.0 / 28.0).y - (1.1 / (1.0 + sin(30.0 / 28.0 * 100.0) * (30.0 / 28.0) * 0.01))).abs() < 1e-5);
    }

    #[test]
    fn the_creeper_flashes_white_on_odd_tenths_with_the_or_ed_alpha() {
        assert_eq!(creeper_flash(0.05), 0.0, "tenth 0 is even");
        assert_eq!(creeper_flash(0.25), 0.0);
        // tenth 1: (int)(0.15 * 0.2 * 255) = 7, | 0x30 = 55
        assert!((creeper_flash(0.15) - 55.0 / 255.0).abs() < 1e-6);
        // tenth 9: (int)(0.95 * 0.2 * 255) = 48 = 0x30, | 0x30 = 48
        assert!((creeper_flash(0.95) - 48.0 / 255.0).abs() < 1e-6);
        // tenth 10 is even again, so a full fuse ends with no flash
        assert_eq!(creeper_flash(30.0 / 28.0), 0.0);
    }

    #[test]
    fn the_creeper_fuse_counts_to_thirty_and_the_flash_is_over_twenty_eight() {
        let mut a = super::super::anim::Anim::default();
        for _ in 0..40 {
            a.tick_creeper(true);
        }
        assert_eq!(a.ignite_time, 30);
        assert!((a.flash_intensity(1.0) - 30.0 / 28.0).abs() < 1e-6);
        a.tick_creeper(false);
        assert_eq!((a.last_ignite, a.ignite_time), (30, 29));
        for _ in 0..40 {
            a.tick_creeper(false);
        }
        assert_eq!(a.ignite_time, 0);
    }

    #[test]
    fn the_spider_legs_spread_at_rest_and_the_eyes_layer_shares_the_body() {
        let a = spider_pose(&frame());
        assert_eq!(a.len(), 11);
        assert_eq!((a[3].y, a[3].z), (0.785_398_2, -0.785_398_2));
        assert_eq!((a[4].y, a[4].z), (-0.785_398_2, 0.785_398_2));
        assert_eq!((a[5].y, a[5].z), (0.392_699_1, -0.581_194_64));
        let moving = spider_pose(&Frame { limb_swing: 1.0, limb_amount: 1.0, ..frame() });
        assert!((moving[3].y - (0.785_398_2 - cos(0.6662 * 2.0) * 0.4)).abs() < 1e-5);
        assert!((moving[3].z - (-0.785_398_2 + (sin(0.6662) * 0.4).abs())).abs() < 1e-5);
        assert_eq!(SPIDER.layers[0].kind, LayerKind::SpiderEyes);
        assert!(LayerKind::SpiderEyes.shown(0) && LayerKind::SpiderEyes.additive() && !LayerKind::SpiderEyes.combines());
        assert!(!LayerKind::CreeperCharge.shown(0) && LayerKind::CreeperCharge.shown(proto::MM_POWERED));
        assert_eq!(SPIDER.death_max, 180.0);
    }

    #[test]
    fn the_charge_layer_scrolls_the_texture_a_hundredth_per_tick() {
        let fx = LayerKind::CreeperCharge.fx(50.0);
        assert_eq!((fx.x, fx.y, fx.z, fx.w), (0.5, 0.5, 0.5, 3.0));
        assert_eq!(LayerKind::SpiderEyes.fixed_light(), Some((240.0, 0.0)));
    }
}
