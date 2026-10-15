//! The mob models as data: `MOBS` is the table, one [`MobDef`] per kind (its parts as
//! `ModelRenderer` boxes, its texture, its `setRotationAngles`, its child layout), so a further
//! mob is one entry here and nothing else. Today: `ModelCow` over `ModelQuadruped`, in
//! `farm.rs` the pig, sheep and chicken, and in `hostile.rs` the zombie, skeleton, creeper and
//! spider.
//!
//! Part order is the order the pose function fills: for a quadruped head, body, leg1 to leg4.
//! Each part's box list carries its own texture offset (`ModelRenderer.setTextureOffset`).
//!
//! DROPPED: `ModelBase.setLivingAnimations` as a call (the sheep's values come in the `Frame`),
//! the `isRiding` field, `RenderLiving`'s shadow, and `LayerSheepWool`'s `jeb_` rainbow.

use std::f32::consts::PI;

use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use worldgen::math_helper::cos;

use super::{farm, hostile};
use super::living::Frame;
use crate::hand::BoxMesh;

/// One `ModelRenderer.addBox` with the texture offset the renderer had at the time.
pub struct BoxDef {
    pub uv: (i32, i32),
    pub min: [f32; 3],
    pub size: [i32; 3],
    /// `addBox`'s scale (the delta that grows the box on every side)
    pub grow: f32,
}

/// One `ModelRenderer`: its rotation point and boxes (a scale/delta of 0, as every cow box has).
pub struct PartDef {
    pub pivot: [f32; 3],
    pub boxes: &'static [BoxDef],
}

/// How a model lays out a baby (`ModelQuadruped.render`'s `isChild` branch). The head moves by
/// `(0, head.0, head.1)` model units; every other part is scaled by 0.5 after a drop of `body_drop`.
pub struct ChildLayout {
    pub head_offset: [f32; 2],
    pub body_drop: f32,
    /// The head's scale: 1 for a quadruped, `ModelBiped.render`'s 0.75 (with the offset taken
    /// under that scale)
    pub head_scale: f32,
}

/// `LayerHeldItem`'s hand: which part is the right arm, and `postRenderArm`'s extra x on its point
/// (`ModelSkeleton` adds 1 for the right side).
pub struct Hand {
    pub part: usize,
    pub dx: f32,
}

/// `MobMeta::flags` bit for a saddled pig (`EntityPig.SADDLED`). Not in `proto` yet: the server
/// has to send it (bit 64 is the next free one, "New flags take free bits").
pub const MM_SADDLED: u8 = 64;

/// Which extra model a layer is, and so when it shows and how its texture is tinted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LayerKind {
    /// `LayerSaddle`: shown while `getSaddled()`
    Saddle,
    /// `LayerSheepWool`: shown while `!getSheared()`, tinted by the fleece color
    Wool,
    /// `LayerCreeperCharge`: shown while `getPowered()`, additive, grey 0.5, the texture scrolling
    CreeperCharge,
    /// `LayerSpiderEyes`: always, additive and lit at the lightmap's (15, 0)
    SpiderEyes,
}

/// One `LayerRenderer`: its own model (the same part order as the base, sharing its pose) and
/// texture.
pub struct LayerDef {
    pub kind: LayerKind,
    pub texture: &'static str,
    pub parts: &'static [PartDef],
}

impl LayerKind {
    /// How many texture variants the layer needs: one per fleece color for wool.
    pub fn variants(self) -> usize {
        match self {
            LayerKind::Wool => 16,
            _ => 1,
        }
    }

    /// `LayerRenderer.shouldCombineTextures`: whether the hurt and flash overlay covers the layer.
    pub fn combines(self) -> bool {
        matches!(self, LayerKind::Saddle | LayerKind::Wool)
    }

    /// Drawn with `blendFunc(ONE, ONE)`.
    pub fn additive(self) -> bool {
        matches!(self, LayerKind::CreeperCharge | LayerKind::SpiderEyes)
    }

    /// The layer's `EntityMaterial::fx` for a model of this age (`ticksExisted + partialTicks`).
    pub fn fx(self, age: f32) -> Vec4 {
        match self {
            // texture matrix translate(f * 0.01, f * 0.01), color(0.5), disableLighting, black fog
            LayerKind::CreeperCharge => Vec4::new(age * 0.01, age * 0.01, 0.5, 3.0),
            // fog toward black only; the normal lighting stays on
            LayerKind::SpiderEyes => Vec4::new(0.0, 0.0, 0.0, 2.0),
            _ => Vec4::ZERO,
        }
    }

    /// The fixed lightmap coordinates of a layer that sets its own (`setLightmapTextureCoords(61680)`
    /// is u 61680, v 0: clamped to block light 15 over sky light 0).
    pub fn fixed_light(self) -> Option<(f32, f32)> {
        match self {
            LayerKind::SpiderEyes => Some((240.0, 0.0)),
            _ => None,
        }
    }

    /// Whether the layer is drawn for a mob with these `MobMeta` flags.
    pub fn shown(self, flags: u8) -> bool {
        match self {
            LayerKind::Saddle => flags & MM_SADDLED != 0,
            LayerKind::Wool => flags & proto::MM_SHEARED == 0,
            LayerKind::CreeperCharge => flags & proto::MM_POWERED != 0,
            LayerKind::SpiderEyes => true,
        }
    }

    /// Which variant of the layer's textures a mob with this fleece color uses.
    pub fn variant(self, wool: u8) -> usize {
        match self {
            LayerKind::Wool => (wool & 15) as usize,
            _ => 0,
        }
    }
}

/// A mob's own sounds: `getHurtSound`, `getDeathSound` and `getSoundVolume`.
pub struct MobSounds {
    pub hurt: &'static str,
    pub death: &'static str,
    pub volume: f32,
}

/// Everything the viewer needs to draw one kind of mob.
pub struct MobDef {
    /// The `EntityList` id of `S_SPAWN_MOB` (`proto::MOB_*`)
    pub kind: u16,
    pub name: &'static str,
    /// The jar path of the texture (`getEntityTexture`)
    pub texture: &'static str,
    /// `ModelBase.textureWidth` and `textureHeight`
    pub tex_size: (f32, f32),
    pub parts: &'static [PartDef],
    /// Indices into `parts` of the parts `render` moves for a child (the head; the chicken's head,
    /// bill and chin). Part 0 is the head for every entry, which `Frame::head_dy` shifts.
    pub heads: &'static [usize],
    pub child: ChildLayout,
    /// `setRotationAngles`: the rotation (x, y, z, radians) of each part
    pub pose: fn(&Frame) -> Vec<Vec3>,
    /// `ModelRenderer.mirror`: the indices into `parts` built mirrored (a biped's left limbs)
    pub mirrored: &'static [usize],
    /// The parts' `rotationPoint` changes `setRotationAngles` makes, added to `PartDef::pivot`
    /// (the arms of a swinging biped)
    pub shift: Option<fn(&Frame) -> Vec<Vec3>>,
    /// `RenderLivingBase.getDeathMaxRotation`
    pub death_max: f32,
    /// `LayerHeldItem`: the arm the main hand item is drawn on, when the renderer has the layer
    pub hand: Option<Hand>,
    /// `Entity.getEyeHeight`, adult and child, which `getBrightnessForRender` lights from
    pub eye_height: (f32, f32),
    /// `Entity.setSize` width and height, for the nameplate and culling later
    pub size: (f32, f32),
    pub sounds: MobSounds,
    /// `RenderLiving.layerRenderers`
    pub layers: &'static [LayerDef],
}

/// `ModelQuadruped.setRotationAngles`
pub fn quadruped_pose(f: &Frame) -> Vec<Vec3> {
    let rad = 0.017_453_292;
    let swing = f.limb_swing * 0.6662;
    let a = f.limb_amount;
    vec![
        Vec3::new(f.head_pitch * rad, f.net_head_yaw * rad, 0.0),
        Vec3::new(1.570_796_4, 0.0, 0.0),
        Vec3::new(cos(swing) * 1.4 * a, 0.0, 0.0),
        Vec3::new(cos(swing + PI) * 1.4 * a, 0.0, 0.0),
        Vec3::new(cos(swing + PI) * 1.4 * a, 0.0, 0.0),
        Vec3::new(cos(swing) * 1.4 * a, 0.0, 0.0),
    ]
}

/// `ModelQuadruped(12, 0)` with `ModelCow`'s overrides: the head with horns, the udder on the
/// body, and the legs moved by `--rotationPointX`, `++rotationPointX` and `--rotationPointZ`.
const LEG: &[BoxDef] = &[BoxDef { uv: (0, 16), min: [-2.0, 0.0, -2.0], size: [4, 12, 4], grow: 0.0 }];
const COW_PARTS: &[PartDef] = &[
    // head: addBox(-4, -4, -6, 8, 8, 6) at (0, 0), the two horns at (22, 0); point (0, 4, -8)
    PartDef {
        pivot: [0.0, 4.0, -8.0],
        boxes: &[
            BoxDef { uv: (0, 0), min: [-4.0, -4.0, -6.0], size: [8, 8, 6], grow: 0.0 },
            BoxDef { uv: (22, 0), min: [-5.0, -5.0, -4.0], size: [1, 3, 1], grow: 0.0 },
            BoxDef { uv: (22, 0), min: [4.0, -5.0, -4.0], size: [1, 3, 1], grow: 0.0 },
        ],
    },
    // body: addBox(-6, -10, -7, 12, 18, 10) at (18, 4), the udder at (52, 0); point (0, 5, 2)
    PartDef {
        pivot: [0.0, 5.0, 2.0],
        boxes: &[
            BoxDef { uv: (18, 4), min: [-6.0, -10.0, -7.0], size: [12, 18, 10], grow: 0.0 },
            BoxDef { uv: (52, 0), min: [-2.0, 2.0, -8.0], size: [4, 6, 1], grow: 0.0 },
        ],
    },
    // legs at (24 - 12) high: leg1 (-3, 12, 7) - 1 x, leg2 (3, 12, 7) + 1 x, leg3 and leg4 the
    // same at z 7 - 1
    PartDef { pivot: [-4.0, 12.0, 7.0], boxes: LEG },
    PartDef { pivot: [4.0, 12.0, 7.0], boxes: LEG },
    PartDef { pivot: [-4.0, 12.0, 6.0], boxes: LEG },
    PartDef { pivot: [4.0, 12.0, 6.0], boxes: LEG },
];

/// The kinds the viewer draws. A new mob is one entry: its parts, texture and pose.
pub const MOBS: &[MobDef] = &[MobDef {
    kind: proto::MOB_COW,
    name: "cow",
    // RenderCow.COW_TEXTURES
    texture: "textures/entity/cow/cow.png",
    // ModelBase's default 64 x 32
    tex_size: (64.0, 32.0),
    parts: COW_PARTS,
    heads: &[0],
    // ModelQuadruped childYOffset 8, ModelCow childZOffset 4 + 2; the rest drops 24 under 0.5
    child: ChildLayout { head_offset: [8.0, 6.0], body_drop: 24.0, head_scale: 1.0 },
    pose: quadruped_pose,
    mirrored: &[],
    shift: None,
    death_max: 90.0,
    hand: None,
    // EntityCow.getEyeHeight: 1.3 adult, `height` for a child (0.7 at half of 1.4)
    eye_height: (1.3, 0.7),
    // EntityCow.setSize(0.9, 1.4)
    size: (0.9, 1.4),
    // EntityCow: ENTITY_COW_HURT and ENTITY_COW_DEATH at getSoundVolume 0.4
    sounds: MobSounds { hurt: "entity.cow.hurt", death: "entity.cow.death", volume: 0.4 },
    // RenderCow has no layers
    layers: &[],
},
MobDef {
    kind: proto::MOB_PIG,
    name: "pig",
    // RenderPig.PIG_TEXTURES
    texture: "textures/entity/pig/pig.png",
    tex_size: (64.0, 32.0),
    parts: farm::PIG_PARTS,
    heads: &[0],
    // ModelPig childYOffset 4, childZOffset left at 4
    child: ChildLayout { head_offset: [4.0, 4.0], body_drop: 24.0, head_scale: 1.0 },
    pose: quadruped_pose,
    mirrored: &[],
    shift: None,
    death_max: 90.0,
    hand: None,
    // Entity.getEyeHeight is height * 0.85 (EntityPig.setSize(0.9, 0.9)); a child is 0.45 high
    eye_height: (0.9 * 0.85, 0.9 * 0.5 * 0.85),
    size: (0.9, 0.9),
    // EntityPig: ENTITY_PIG_HURT and ENTITY_PIG_DEATH at the default getSoundVolume 1
    sounds: MobSounds { hurt: "entity.pig.hurt", death: "entity.pig.death", volume: 1.0 },
    layers: &[LayerDef { kind: LayerKind::Saddle, texture: "textures/entity/pig/pig_saddle.png", parts: farm::PIG_SADDLE_PARTS }],
},
MobDef {
    kind: proto::MOB_SHEEP,
    name: "sheep",
    // RenderSheep.SHEARED_SHEEP_TEXTURES
    texture: "textures/entity/sheep/sheep.png",
    tex_size: (64.0, 32.0),
    parts: farm::SHEEP_PARTS,
    heads: &[0],
    // ModelQuadruped's 8 and 4 (ModelSheep1 and 2 leave them)
    child: ChildLayout { head_offset: [8.0, 4.0], body_drop: 24.0, head_scale: 1.0 },
    pose: farm::sheep_pose,
    mirrored: &[],
    shift: None,
    death_max: 90.0,
    hand: None,
    // EntitySheep.getEyeHeight: 0.95 * height (setSize(0.9, 1.3)); a child is 0.65 high
    eye_height: (0.95 * 1.3, 0.95 * 1.3 * 0.5),
    size: (0.9, 1.3),
    sounds: MobSounds { hurt: "entity.sheep.hurt", death: "entity.sheep.death", volume: 1.0 },
    layers: &[LayerDef { kind: LayerKind::Wool, texture: "textures/entity/sheep/sheep_fur.png", parts: farm::SHEEP_WOOL_PARTS }],
},
MobDef {
    kind: proto::MOB_CHICKEN,
    name: "chicken",
    // RenderChicken.CHICKEN_TEXTURES
    texture: "textures/entity/chicken.png",
    tex_size: (64.0, 32.0),
    parts: farm::CHICKEN_PARTS,
    // ModelChicken.render moves the head, bill and chin by (0, 5, 2) for a child
    heads: &[0, 1, 2],
    child: ChildLayout { head_offset: [5.0, 2.0], body_drop: 24.0, head_scale: 1.0 },
    pose: farm::chicken_pose,
    mirrored: &[],
    shift: None,
    death_max: 90.0,
    hand: None,
    // EntityChicken.getEyeHeight is `height` (setSize(0.4, 0.7)); a child is 0.35 high
    eye_height: (0.7, 0.7 * 0.5),
    size: (0.4, 0.7),
    sounds: MobSounds { hurt: "entity.chicken.hurt", death: "entity.chicken.death", volume: 1.0 },
    layers: &[],
},
hostile::ZOMBIE,
hostile::SKELETON,
hostile::CREEPER,
hostile::SPIDER];

/// The table entry for an `EntityList` id.
pub fn def_of(kind: u16) -> Option<(usize, &'static MobDef)> {
    MOBS.iter().enumerate().find(|(_, d)| d.kind == kind)
}

/// A part's mesh: its boxes on the model's texture size, unmirrored.
pub fn part_mesh(def: &MobDef, part: &PartDef, mirror: bool) -> Mesh {
    let mut b = BoxMesh::default();
    for x in part.boxes {
        let [mx, my, mz] = x.min;
        let [dx, dy, dz] = x.size;
        b.model_box_mirrored(def.tex_size.0, def.tex_size.1, x.uv.0, x.uv.1, mx, my, mz, dx, dy, dz, x.grow, mirror);
    }
    Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::RENDER_WORLD)
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, b.pos)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, b.normal)
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, b.uv)
        .with_inserted_indices(Indices::U32(b.idx))
}

/// Each part's local transform in model units: `ModelRenderer.render` (translate to the point,
/// rotate Z, Y, X) under `ModelQuadruped.render`'s child branch when `child`.
pub fn part_transforms(def: &MobDef, frame: &Frame, child: bool) -> Vec<Transform> {
    layer_transforms(def, def.parts, frame, child)
}

/// [`part_transforms`] for any model of the same layout (a layer shares the base's pose).
pub fn layer_transforms(def: &MobDef, parts: &[PartDef], frame: &Frame, child: bool) -> Vec<Transform> {
    let angles = (def.pose)(frame);
    let shifts = def.shift.map(|f| f(frame));
    parts
        .iter()
        .zip(angles)
        .enumerate()
        .map(|(i, (p, a))| {
            let rotation = Quat::from_euler(EulerRot::ZYX, a.z, a.y, a.x);
            // Part 0's point moves with the sheep's eating head (`head.rotationPointY`).
            let mut pivot = Vec3::from_array(p.pivot) + if i == 0 { Vec3::new(0.0, frame.head_dy, 0.0) } else { Vec3::ZERO };
            if let Some(shift) = shifts.as_ref().and_then(|s| s.get(i)) {
                pivot += *shift;
            }
            match (child, def.heads.contains(&i)) {
                // translate(0, childYOffset * scale, childZOffset * scale), then the head's own render;
                // a biped scales the head first (0.75) and so moves it under that scale
                (true, true) => {
                    let s = def.child.head_scale;
                    Transform { translation: (pivot + Vec3::new(0.0, def.child.head_offset[0], def.child.head_offset[1])) * s, rotation, scale: Vec3::splat(s) }
                }
                // scale(0.5), translate(0, 24 * scale, 0), then the part's own render
                (true, false) => Transform { translation: (pivot + Vec3::new(0.0, def.child.body_drop, 0.0)) * 0.5, rotation, scale: Vec3::splat(0.5) },
                (false, _) => Transform { translation: pivot, rotation, scale: Vec3::ONE },
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(limb_swing: f32, limb_amount: f32) -> Frame {
        Frame { body_yaw: 0.0, net_head_yaw: 0.0, head_pitch: 0.0, age: 0.0, limb_swing, limb_amount, head_dy: 0.0, head_x: None, ..Default::default() }
    }

    #[test]
    fn idle_legs_are_straight_and_the_body_lies_flat() {
        let a = quadruped_pose(&frame(7.0, 0.0));
        assert_eq!(a.len(), 6);
        assert!(a[2..].iter().all(|v| v.x == 0.0));
        assert_eq!(a[1].x, 1.570_796_4);
    }

    #[test]
    fn diagonal_legs_swing_together() {
        let a = quadruped_pose(&frame(1.0, 1.0));
        let want = cos(0.6662) * 1.4;
        let other = cos(0.6662 + PI) * 1.4;
        assert!((a[2].x - want).abs() < 1e-6 && (a[5].x - want).abs() < 1e-6, "leg1 and leg4: {:?}", a);
        assert!((a[3].x - other).abs() < 1e-6 && (a[4].x - other).abs() < 1e-6, "leg2 and leg3");
        assert!((a[2].x + a[3].x).abs() < 1e-3, "opposite legs mirror");
    }

    #[test]
    fn the_head_follows_pitch_and_net_yaw_in_radians() {
        let f = Frame { net_head_yaw: 90.0, head_pitch: 45.0, ..frame(0.0, 0.0) };
        let a = quadruped_pose(&f);
        assert!((a[0].y - PI / 2.0).abs() < 1e-5 && (a[0].x - PI / 4.0).abs() < 1e-5);
    }

    #[test]
    fn the_cow_legs_sit_where_modelcow_moves_them() {
        let pivots: Vec<[f32; 3]> = COW_PARTS.iter().map(|p| p.pivot).collect();
        assert_eq!(pivots[2], [-4.0, 12.0, 7.0]);
        assert_eq!(pivots[3], [4.0, 12.0, 7.0]);
        assert_eq!(pivots[4], [-4.0, 12.0, 6.0]);
        assert_eq!(pivots[5], [4.0, 12.0, 6.0]);
    }

    #[test]
    fn a_child_moves_the_head_and_halves_the_rest() {
        let (_, cow) = def_of(proto::MOB_COW).unwrap();
        let adult = part_transforms(cow, &frame(0.0, 0.0), false);
        let baby = part_transforms(cow, &frame(0.0, 0.0), true);
        assert_eq!(baby[0].translation - adult[0].translation, Vec3::new(0.0, 8.0, 6.0));
        assert_eq!(baby[0].scale, Vec3::ONE);
        assert_eq!(baby[2].scale, Vec3::splat(0.5));
        assert_eq!(baby[2].translation, (adult[2].translation + Vec3::new(0.0, 24.0, 0.0)) * 0.5);
    }

    #[test]
    fn a_chicken_child_moves_its_head_bill_and_chin_by_five_and_two() {
        let (_, hen) = def_of(proto::MOB_CHICKEN).unwrap();
        let adult = part_transforms(hen, &frame(0.0, 0.0), false);
        let baby = part_transforms(hen, &frame(0.0, 0.0), true);
        for i in 0..3 {
            assert_eq!(baby[i].translation - adult[i].translation, Vec3::new(0.0, 5.0, 2.0));
            assert_eq!(baby[i].scale, Vec3::ONE);
        }
        for i in 3..8 {
            assert_eq!(baby[i].scale, Vec3::splat(0.5));
            assert_eq!(baby[i].translation, (adult[i].translation + Vec3::new(0.0, 24.0, 0.0)) * 0.5);
        }
    }

    #[test]
    fn the_pig_child_head_offset_is_four_and_four() {
        let (_, pig) = def_of(proto::MOB_PIG).unwrap();
        let adult = part_transforms(pig, &frame(0.0, 0.0), false);
        let baby = part_transforms(pig, &frame(0.0, 0.0), true);
        assert_eq!(baby[0].translation - adult[0].translation, Vec3::new(0.0, 4.0, 4.0));
    }

    #[test]
    fn the_eating_sheep_head_drops_nine_units_at_full_dip_and_the_layer_follows() {
        let (_, sheep) = def_of(proto::MOB_SHEEP).unwrap();
        let f = Frame { head_dy: 9.0, head_x: Some(0.628), ..frame(0.0, 0.0) };
        let body = part_transforms(sheep, &f, false);
        assert_eq!(body[0].translation, Vec3::new(0.0, 15.0, -8.0));
        let wool = layer_transforms(sheep, sheep.layers[0].parts, &f, false);
        assert_eq!(wool[0].translation, body[0].translation);
        assert_eq!(wool[1].translation, body[1].translation, "only part 0 moves");
        assert!((body[0].rotation.to_euler(EulerRot::ZYX).2 - 0.628).abs() < 1e-5);
    }

    #[test]
    fn layers_show_by_the_flags_and_pick_their_texture_variant() {
        assert!(LayerKind::Wool.shown(0) && !LayerKind::Wool.shown(proto::MM_SHEARED | proto::MM_CHILD));
        assert!(!LayerKind::Saddle.shown(proto::MM_CHILD) && LayerKind::Saddle.shown(MM_SADDLED));
        assert_eq!((LayerKind::Wool.variants(), LayerKind::Wool.variant(14), LayerKind::Saddle.variant(14)), (16, 14, 0));
    }

    #[test]
    fn layer_parts_match_their_base_in_count_and_points() {
        for d in MOBS {
            for l in d.layers {
                assert_eq!(l.parts.len(), d.parts.len(), "{}", d.name);
                assert!(l.parts.iter().zip(d.parts).all(|(a, b)| a.pivot == b.pivot), "{}", d.name);
            }
        }
    }

    #[test]
    fn every_entry_builds_meshes_in_the_texture() {
        for d in MOBS {
            assert!(def_of(d.kind).is_some());
            for p in d.parts.iter().chain(d.layers.iter().flat_map(|l| l.parts)) {
                let m = part_mesh(d, p, false);
                let Some(bevy::mesh::VertexAttributeValues::Float32x2(uv)) = m.attribute(Mesh::ATTRIBUTE_UV_0) else { panic!("uv") };
                assert_eq!(uv.len(), p.boxes.len() * 24);
                assert!(uv.iter().all(|u| (0.0..=1.0).contains(&u[0]) && (0.0..=1.0).contains(&u[1])), "{} outside its texture", d.name);
            }
        }
    }
}
