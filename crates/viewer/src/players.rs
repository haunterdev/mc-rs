//! Other players in a shared world (streamed sessions, link.rs): `EntityOtherPlayerMP` moving
//! toward the positions the server relays, drawn as `ModelPlayer` with the Steve skin under
//! `RenderPlayer`'s transforms, with the name over the head as `RenderLivingBase.renderName`
//! draws it.
//!
//! The model's parts are child entities, each a box and its overlay layer, posed per frame by
//! `ModelBiped.setRotationAngles` and lit as vanilla lights entities (shaders/entity.wgsl). The
//! name is `EntityRenderer.drawNameplate` in the game's own font: a billboard over the head, its
//! background and a faint copy seen through walls, the text itself depth tested; a sneaking
//! player's name is depth tested only and dim. In third person (F5) the player's own model
//! draws too, following the player each tick.

use std::collections::HashMap;
use std::f32::consts::PI;

use bevy::asset::RenderAssetUsages;
use bevy::math::DVec3;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use proto::PlayerState;
use worldgen::math_helper::sqrt_f;

use bevy::camera::visibility::NoFrustumCulling;
use bevy::light::NotShadowCaster;

use crate::hand::{BoxMesh, Hand, HideGui};
use crate::hotbar::Inventory;
use crate::material::{EntityMaterial, NameplateMaterial, PlateKind, PlateParams};
use crate::player::Player;
use crate::sky::{neighbor_light, Clock, View};
use crate::stream::Streamer;
use crate::textures::Argb;

/// What link.rs hands over from the server.
pub enum OtherEvent {
    Join(u32, String),
    Leave(u32),
    State(u32, PlayerState),
}

#[derive(Resource, Default)]
pub struct Others {
    pub events: Vec<OtherEvent>,
    by_id: HashMap<u32, Entity>,
}

/// `Entity.distanceWalkedOnStepModified` for another player: the server's `Entity.move` plays
/// their steps for everyone else, which this stands in for.
fn step_due(o: &mut Other, d: DVec3) -> bool {
    let flags = o.target.flags;
    if flags & proto::FLYING != 0 || flags & proto::ON_GROUND == 0 || flags & proto::SNEAKING != 0 {
        return false;
    }
    o.dist_on_step += ((d.x * d.x + d.z * d.z).sqrt() * 0.6) as f32;
    if o.dist_on_step > o.next_step as f32 {
        o.next_step = o.dist_on_step as i32 + 1;
        return true;
    }
    false
}

/// `textures/entity/steve.png` (in the blocks' gamma space, as the hand reads it) and the
/// environment texture, set up by main.rs.
#[derive(Resource)]
pub struct Skin {
    pub skin: Handle<Image>,
    pub env: Handle<Image>,
}

/// `textures/font/ascii.png` and `FontRenderer.charWidth`.
#[derive(Resource)]
pub struct Font {
    pub image: Handle<Image>,
    pub widths: [u8; 256],
}

impl Font {
    /// `FontRenderer.readFontTexture`: each glyph's width is its last column with any opaque
    /// pixel, plus one, scaled to 8 pixels a cell, plus one.
    pub fn widths(img: &Argb) -> [u8; 256] {
        let (w, h) = (img.width as usize, img.height as usize);
        let (cw, ch) = (w / 16, h / 16);
        let scale = 8.0 / cw as f32;
        std::array::from_fn(|i| {
            let (gx, gy) = (i % 16, i / 16);
            let mut l1 = cw as i32 - 1;
            while l1 >= 0 {
                let x = gx * cw + l1 as usize;
                let opaque = (0..ch).any(|y| img.pixels[(gy * ch + y) * w + x] >> 24 != 0);
                if opaque {
                    break;
                }
                l1 -= 1;
            }
            let l1 = l1 + 1;
            if i == 32 {
                // A space is 4 wide.
                return 4;
            }
            ((0.5 + (l1 as f32 * scale) as f64) as i32 + 1) as u8
        })
    }

    /// `FontRenderer.getStringWidth`
    pub fn string_width(&self, s: &str) -> i32 {
        s.bytes().map(|b| self.widths[b as usize] as i32).sum()
    }
}

/// The name tag materials, one per way a piece draws.
#[derive(Resource)]
pub struct PlateMaterials {
    bg: Handle<NameplateMaterial>,
    bg_depth: Handle<NameplateMaterial>,
    through: Handle<NameplateMaterial>,
    text: Handle<NameplateMaterial>,
    text_dim: Handle<NameplateMaterial>,
}

/// A name tag's three pieces.
#[derive(Clone, Copy)]
struct Plate {
    bg: Entity,
    through: Entity,
    text: Entity,
}

/// `SWING_END` in hand.rs.
const SWING_END: i32 = 6;

/// One other player.
#[derive(Component)]
pub struct Other {
    /// `otherPlayerMPX/Y/Z/Yaw/Pitch` and `otherPlayerMPPosRotationIncrements`.
    target: PlayerState,
    increments: i32,
    pos: DVec3,
    prev_pos: DVec3,
    yaw: f32,
    prev_yaw: f32,
    pitch: f32,
    prev_pitch: f32,
    /// `renderYawOffset`
    body_yaw: f32,
    prev_body_yaw: f32,
    limb_swing: f32,
    limb_amount: f32,
    prev_limb_amount: f32,
    swing: f32,
    prev_swing: f32,
    swing_int: i32,
    swinging: bool,
    swings_seen: Option<u8>,
    /// `ticksExisted`
    age: u32,
    dist_on_step: f32,
    next_step: i32,
    plate: Option<Plate>,
    parts: [Entity; 6],
    material: Handle<EntityMaterial>,
}

/// Marks the player's own model (third person).
#[derive(Component)]
pub struct OwnModel;

#[derive(Clone, Copy)]
enum Part {
    Head,
    Body,
    RightArm,
    LeftArm,
    RightLeg,
    LeftLeg,
}

const PARTS: [Part; 6] = [Part::Head, Part::Body, Part::RightArm, Part::LeftArm, Part::RightLeg, Part::LeftLeg];

/// `ModelPlayer(0, false)`: each part's box and overlay (texture offsets, box, overlay delta).
fn part_mesh(part: Part) -> Mesh {
    let mut b = BoxMesh::default();
    let (base, over, origin, size, delta) = match part {
        Part::Head => ((0, 0), (32, 0), (-4.0, -8.0, -4.0), (8, 8, 8), 0.5),
        Part::Body => ((16, 16), (16, 32), (-4.0, 0.0, -2.0), (8, 12, 4), 0.25),
        Part::RightArm => ((40, 16), (40, 32), (-3.0, -2.0, -2.0), (4, 12, 4), 0.25),
        Part::LeftArm => ((32, 48), (48, 48), (-1.0, -2.0, -2.0), (4, 12, 4), 0.25),
        Part::RightLeg => ((0, 16), (0, 32), (-2.0, 0.0, -2.0), (4, 12, 4), 0.25),
        Part::LeftLeg => ((16, 48), (0, 48), (-2.0, 0.0, -2.0), (4, 12, 4), 0.25),
    };
    let (x, y, z) = origin;
    b.model_box(base.0, base.1, x, y, z, size.0, size.1, size.2, 0.0);
    b.model_box(over.0, over.1, x, y, z, size.0, size.1, size.2, delta);
    Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::RENDER_WORLD)
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, b.pos)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, b.normal)
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, b.uv)
        .with_inserted_indices(Indices::U32(b.idx))
}

/// A name tag's background quad and its text, in the font's pixels (y down), centered as
/// `drawNameplate` centers them: the text from `-width / 2`, the quad one pixel wider each side,
/// from -1 to 8 tall.
fn plate_meshes(font: &Font, name: &str) -> (Mesh, Mesh) {
    let half = font.string_width(name) / 2;
    let quad_mesh = |quads: &[([f32; 4], [f32; 4])]| {
        let (mut pos, mut uv, mut idx) = (Vec::new(), Vec::new(), Vec::new());
        for ([x0, y0, x1, y1], [u0, v0, u1, v1]) in quads.iter().copied() {
            let base = pos.len() as u32;
            pos.extend_from_slice(&[[x0, y0, 0.0], [x0, y1, 0.0], [x1, y1, 0.0], [x1, y0, 0.0]]);
            uv.extend_from_slice(&[[u0, v0], [u0, v1], [u1, v1], [u1, v0]]);
            idx.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
        }
        let n = vec![[0.0, 0.0, 1.0]; pos.len()];
        Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::RENDER_WORLD)
            .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, pos)
            .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, n)
            .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uv)
            .with_inserted_indices(Indices::U32(idx))
    };
    let bg = quad_mesh(&[([-half as f32 - 1.0, -1.0, half as f32 + 1.0, 8.0], [0.0; 4])]);
    // FontRenderer.renderDefaultChar: a glyph quad charWidth - 1.01 wide, 7.99 tall.
    let mut glyphs = Vec::new();
    let mut x = -half as f32;
    for b in name.bytes() {
        let w = font.widths[b as usize] as f32;
        let f = w - 0.01;
        let (gx, gy) = ((b % 16) as f32 * 8.0, (b / 16) as f32 * 8.0);
        glyphs.push(([x, 0.0, x + f - 1.0, 7.99], [gx / 128.0, gy / 128.0, (gx + f - 1.0) / 128.0, (gy + 7.99) / 128.0]));
        x += w;
    }
    (bg, quad_mesh(&glyphs))
}

#[derive(Resource)]
pub struct PartMeshes([Handle<Mesh>; 6]);

/// `MathHelper.wrapDegrees`
fn wrap(mut a: f32) -> f32 {
    a %= 360.0;
    if a >= 180.0 {
        a -= 360.0;
    }
    if a < -180.0 {
        a += 360.0;
    }
    a
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

impl Other {
    fn new(plate: Option<Plate>, parts: [Entity; 6], material: Handle<EntityMaterial>) -> Other {
        Other {
            target: PlayerState::default(),
            increments: -1,
            pos: DVec3::ZERO,
            prev_pos: DVec3::ZERO,
            yaw: 0.0,
            prev_yaw: 0.0,
            pitch: 0.0,
            prev_pitch: 0.0,
            body_yaw: 0.0,
            prev_body_yaw: 0.0,
            limb_swing: 0.0,
            limb_amount: 0.0,
            prev_limb_amount: 0.0,
            swing: 0.0,
            prev_swing: 0.0,
            swing_int: 0,
            swinging: false,
            swings_seen: None,
            age: 0,
            dist_on_step: 0.0,
            next_step: 1,
            plate,
            parts,
            material,
        }
    }

    /// `EntityOtherPlayerMP.setPositionAndRotationDirect` with its three steps; the first state
    /// places the player at once.
    fn set_target(&mut self, s: PlayerState) {
        let first = self.increments < 0;
        self.target = s;
        self.increments = 3;
        if first {
            self.pos = DVec3::from_array(s.pos);
            self.prev_pos = self.pos;
            (self.yaw, self.prev_yaw, self.body_yaw, self.prev_body_yaw) = (s.yaw, s.yaw, s.yaw, s.yaw);
            (self.pitch, self.prev_pitch) = (s.pitch, s.pitch);
            self.increments = 0;
        }
        match self.swings_seen {
            Some(n) if n != s.swings => {
                // EntityLivingBase.swingArm
                if !self.swinging || self.swing_int >= SWING_END / 2 || self.swing_int < 0 {
                    self.swing_int = -1;
                    self.swinging = true;
                }
            }
            _ => {}
        }
        self.swings_seen = Some(s.swings);
    }

    /// One client tick: `EntityOtherPlayerMP.onUpdate` and `onLivingUpdate`, then
    /// `EntityLivingBase.onUpdate`'s body turn.
    fn tick(&mut self) {
        self.age += 1;
        self.prev_pos = self.pos;
        self.prev_yaw = self.yaw;
        self.prev_pitch = self.pitch;
        self.prev_body_yaw = self.body_yaw;
        self.prev_limb_amount = self.limb_amount;
        self.prev_swing = self.swing;
        if self.increments > 0 {
            let n = self.increments as f64;
            self.pos += (DVec3::from_array(self.target.pos) - self.pos) / n;
            self.yaw += wrap(self.target.yaw - self.yaw) / self.increments as f32;
            self.pitch += (self.target.pitch - self.pitch) / self.increments as f32;
            self.increments -= 1;
        }
        self.animate();
    }

    /// The player's own model: placed where the player is after this tick, then animated the
    /// same way.
    fn follow(&mut self, s: PlayerState) {
        self.age += 1;
        self.prev_pos = self.pos;
        self.prev_yaw = self.yaw;
        self.prev_pitch = self.pitch;
        self.prev_body_yaw = self.body_yaw;
        self.prev_limb_amount = self.limb_amount;
        self.prev_swing = self.swing;
        let first = self.increments < 0;
        self.set_target(s);
        self.increments = 0;
        if first {
            self.prev_pos = self.pos;
        }
        self.pos = DVec3::from_array(s.pos);
        self.yaw = s.yaw;
        self.pitch = s.pitch;
        self.animate();
    }

    /// `EntityLivingBase.onUpdate`'s limb swing, arm swing and body turn.
    fn animate(&mut self) {
        // onUpdate: the limb swing from how far it moved.
        let d = self.pos - self.prev_pos;
        let f = (sqrt_f((d.x * d.x + d.z * d.z) as f32) * 4.0).min(1.0);
        self.limb_amount += (f - self.limb_amount) * 0.4;
        self.limb_swing += self.limb_amount;
        // updateArmSwingProgress
        if self.swinging {
            self.swing_int += 1;
            if self.swing_int >= SWING_END {
                self.swing_int = 0;
                self.swinging = false;
            }
        } else {
            self.swing_int = 0;
        }
        self.swing = self.swing_int as f32 / SWING_END as f32;
        // EntityLivingBase.onUpdate: the body turns toward the way it moves.
        let dist2 = (d.x * d.x + d.z * d.z) as f32;
        let mut f4 = self.body_yaw;
        if dist2 > 0.0025 {
            f4 = (d.z as f32).atan2(d.x as f32).to_degrees() - 90.0;
        }
        if self.swing > 0.0 {
            f4 = self.yaw;
        }
        // updateDistance
        self.body_yaw += wrap(f4 - self.body_yaw) * 0.3;
        let f1 = wrap(self.yaw - self.body_yaw).clamp(-75.0, 75.0);
        self.body_yaw = self.yaw - f1;
        if f1 * f1 > 2500.0 {
            self.body_yaw += f1 * 0.2;
        }
    }

    fn sneaking(&self) -> bool {
        self.target.flags & proto::SNEAKING != 0
    }

    fn swing_progress(&self, partial: f32) -> f32 {
        let mut f = self.swing - self.prev_swing;
        if f < 0.0 {
            f += 1.0;
        }
        self.prev_swing + f * partial
    }
}

/// The part meshes and name tag materials, made once.
fn shared(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    plates: &mut Assets<NameplateMaterial>,
    font: &Font,
    have: Option<(&PartMeshes, &PlateMaterials)>,
) -> ([Handle<Mesh>; 6], [Handle<NameplateMaterial>; 5]) {
    if let Some((p, m)) = have {
        return (p.0.clone(), [m.bg.clone(), m.bg_depth.clone(), m.through.clone(), m.text.clone(), m.text_dim.clone()]);
    }
    let parts = PARTS.map(|p| meshes.add(part_mesh(p)));
    let mut plate = |kind, color: Vec4, solid: bool| {
        plates.add(NameplateMaterial { font: font.image.clone(), params: PlateParams { color, mode: Vec4::new(solid as u32 as f32, 0.0, 0.0, 0.0) }, kind })
    };
    // drawNameplate's colors: the quad (0, 0, 0, 0.25), the faint text 0x20FFFFFF, the text -1.
    let faint = Vec4::new(1.0, 1.0, 1.0, 32.0 / 255.0);
    let m = PlateMaterials {
        bg: plate(PlateKind::Background, Vec4::new(0.0, 0.0, 0.0, 0.25), true),
        bg_depth: plate(PlateKind::BackgroundDepth, Vec4::new(0.0, 0.0, 0.0, 0.25), true),
        through: plate(PlateKind::TextThrough, faint, false),
        text: plate(PlateKind::Text, Vec4::ONE, false),
        text_dim: plate(PlateKind::Text, faint, false),
    };
    let out = (parts.clone(), [m.bg.clone(), m.bg_depth.clone(), m.through.clone(), m.text.clone(), m.text_dim.clone()]);
    commands.insert_resource(PartMeshes(parts));
    commands.insert_resource(m);
    out
}

/// Spawns a model, and its name tag when it has a name; returns the root.
#[allow(clippy::too_many_arguments)]
fn spawn_model(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<EntityMaterial>,
    skin: &Skin,
    font: &Font,
    parts: &[Handle<Mesh>; 6],
    plate_mats: &[Handle<NameplateMaterial>; 5],
    name: Option<&str>,
) -> Entity {
    let material = materials.add(EntityMaterial { env: skin.env.clone(), skin: skin.skin.clone(), light: Vec4::new(0.0, 240.0, 0.0, 0.0) });
    let root = commands.spawn((Transform::default(), Visibility::Hidden)).id();
    let parts = std::array::from_fn(|i| {
        commands.spawn((Mesh3d(parts[i].clone()), MeshMaterial3d(material.clone()), Transform::default(), ChildOf(root))).id()
    });
    let plate = name.map(|name| {
        let (bg, text) = plate_meshes(font, name);
        let (bg, text) = (meshes.add(bg), meshes.add(text));
        let mut piece = |mesh: &Handle<Mesh>, m: &Handle<NameplateMaterial>| {
            commands.spawn((Mesh3d(mesh.clone()), MeshMaterial3d(m.clone()), Transform::default(), Visibility::Hidden, NoFrustumCulling, NotShadowCaster)).id()
        };
        Plate { bg: piece(&bg, &plate_mats[0]), through: piece(&text, &plate_mats[2]), text: piece(&text, &plate_mats[3]) }
    });
    commands.entity(root).insert(Other::new(plate, parts, material));
    root
}

/// Spawns, drops and retargets players as the server says.
#[allow(clippy::too_many_arguments)]
pub fn apply_events(
    mut commands: Commands,
    mut others: ResMut<Others>,
    skin: Res<Skin>,
    font: Res<Font>,
    have: Option<(Res<PartMeshes>, Res<PlateMaterials>)>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<EntityMaterial>>,
    mut plates: ResMut<Assets<NameplateMaterial>>,
    mut players: Query<&mut Other>,
) {
    if others.events.is_empty() {
        return;
    }
    let (parts, plate_mats) = shared(&mut commands, &mut meshes, &mut plates, &font, have.as_ref().map(|(a, b)| (&**a, &**b)));
    let events = std::mem::take(&mut others.events);
    for e in events {
        match e {
            OtherEvent::Join(id, name) => {
                if others.by_id.contains_key(&id) {
                    continue;
                }
                let root = spawn_model(&mut commands, &mut meshes, &mut materials, &skin, &font, &parts, &plate_mats, Some(&name));
                others.by_id.insert(id, root);
            }
            OtherEvent::Leave(id) => {
                if let Some(e) = others.by_id.remove(&id) {
                    if let Some(p) = players.get(e).ok().and_then(|o| o.plate) {
                        for piece in [p.bg, p.through, p.text] {
                            commands.entity(piece).despawn();
                        }
                    }
                    commands.entity(e).despawn();
                }
            }
            OtherEvent::State(id, s) => {
                if let Some(mut o) = others.by_id.get(&id).and_then(|&e| players.get_mut(e).ok()) {
                    o.set_target(s);
                }
            }
        }
    }
}

/// The player's own model for third person: spawned once, then placed where the player is each
/// tick.
#[allow(clippy::too_many_arguments)]
pub fn own_model(
    mut commands: Commands,
    clock: Res<Clock>,
    player: Res<Player>,
    hand: Res<Hand>,
    inv: Res<Inventory>,
    skin: Res<Skin>,
    font: Res<Font>,
    have: Option<(Res<PartMeshes>, Res<PlateMaterials>)>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<EntityMaterial>>,
    mut plates: ResMut<Assets<NameplateMaterial>>,
    mut own: Query<&mut Other, With<OwnModel>>,
) {
    let Ok(mut o) = own.single_mut() else {
        let (parts, plate_mats) = shared(&mut commands, &mut meshes, &mut plates, &font, have.as_ref().map(|(a, b)| (&**a, &**b)));
        let root = spawn_model(&mut commands, &mut meshes, &mut materials, &skin, &font, &parts, &plate_mats, None);
        commands.entity(root).insert(OwnModel);
        return;
    };
    if clock.ticks_this_frame == 0 || !player.spawned {
        return;
    }
    let mut flags = 0;
    if player.on_ground {
        flags |= proto::ON_GROUND;
    }
    if player.is_sneaking() {
        flags |= proto::SNEAKING;
    }
    if player.flying {
        flags |= proto::FLYING;
    }
    let held = crate::hotbar::held_state(&inv.get_current_item());
    o.follow(PlayerState { pos: player.pos.to_array(), yaw: player.yaw, pitch: player.pitch, flags, swings: hand.swings, held });
}

/// This frame's client ticks for every other player, with their footsteps.
pub fn tick_others(clock: Res<Clock>, streamer: Res<Streamer>, audio: Res<crate::audio::Audio>, mut players: Query<&mut Other, Without<OwnModel>>) {
    for _ in 0..clock.ticks_this_frame {
        for mut o in &mut players {
            o.tick();
            let d = o.pos - o.prev_pos;
            if o.increments >= 0 && step_due(&mut o, d) {
                let p = o.pos;
                let (x, y, z) = (p.x.floor() as i32, (p.y - 0.20000000298023224).floor() as i32, p.z.floor() as i32);
                let s = streamer.state_at(x, y, z).unwrap_or(0);
                if s != 0 && !matches!(s >> 4, 8..=11) {
                    let (name, v, pitch) = crate::sfx::sound_of(s);
                    audio.play(&format!("{name}.step"), crate::audio::Category::Players, Some(p.as_vec3()), v * 0.15, pitch);
                }
            }
        }
    }
}

/// Part pivots and angles from `ModelBiped.setRotationAngles` (the player model's own overrides
/// change nothing for an empty or block-holding hand).
fn pose(o: &Other, partial: f32) -> [(Vec3, Vec3); 6] {
    let limb_amount = lerp(o.prev_limb_amount, o.limb_amount, partial).min(1.0);
    let limb_swing = o.limb_swing - o.limb_amount * (1.0 - partial);
    let age = o.age as f32 + partial;
    let body_yaw = lerp(o.prev_body_yaw, o.body_yaw, partial);
    let yaw = o.prev_yaw + wrap(o.yaw - o.prev_yaw) * partial;
    let head_yaw = wrap(yaw - body_yaw).to_radians();
    let head_pitch = lerp(o.prev_pitch, o.pitch, partial).to_radians();
    let sneak = o.sneaking();
    let swing = o.swing_progress(partial);

    let mut head = (Vec3::ZERO, Vec3::new(head_pitch, head_yaw, 0.0));
    let mut body = (Vec3::ZERO, Vec3::ZERO);
    let mut right_arm = (Vec3::new(-5.0, 2.0, 0.0), Vec3::ZERO);
    let mut left_arm = (Vec3::new(5.0, 2.0, 0.0), Vec3::ZERO);
    let mut right_leg = (Vec3::new(-1.9, 12.0, 0.1), Vec3::ZERO);
    let mut left_leg = (Vec3::new(1.9, 12.0, 0.1), Vec3::ZERO);

    right_arm.1.x = (limb_swing * 0.6662 + PI).cos() * 2.0 * limb_amount * 0.5;
    left_arm.1.x = (limb_swing * 0.6662).cos() * 2.0 * limb_amount * 0.5;
    right_leg.1.x = (limb_swing * 0.6662).cos() * 1.4 * limb_amount;
    left_leg.1.x = (limb_swing * 0.6662 + PI).cos() * 1.4 * limb_amount;
    // ArmPose.ITEM for a held block.
    if o.target.held != 0 {
        right_arm.1.x = right_arm.1.x * 0.5 - PI / 10.0;
    }
    if swing > 0.0 {
        body.1.y = ((swing.sqrt()) * PI * 2.0).sin() * 0.2;
        right_arm.0.z = body.1.y.sin() * 5.0;
        right_arm.0.x = -body.1.y.cos() * 5.0;
        left_arm.0.z = -body.1.y.sin() * 5.0;
        left_arm.0.x = body.1.y.cos() * 5.0;
        right_arm.1.y += body.1.y;
        left_arm.1.y += body.1.y;
        left_arm.1.x += body.1.y;
        let mut f1 = 1.0 - swing;
        f1 *= f1;
        f1 *= f1;
        f1 = 1.0 - f1;
        let f2 = (f1 * PI).sin();
        let f3 = (swing * PI).sin() * -(head.1.x - 0.7) * 0.75;
        right_arm.1.x -= f2 * 1.2 + f3;
        right_arm.1.y += body.1.y * 2.0;
        right_arm.1.z += (swing * PI).sin() * -0.4;
    }
    if sneak {
        body.1.x = 0.5;
        right_arm.1.x += 0.4;
        left_arm.1.x += 0.4;
        right_leg.0.z = 4.0;
        left_leg.0.z = 4.0;
        right_leg.0.y = 9.0;
        left_leg.0.y = 9.0;
        head.0.y = 1.0;
    }
    right_arm.1.z += (age * 0.09).cos() * 0.05 + 0.05;
    left_arm.1.z -= (age * 0.09).cos() * 0.05 + 0.05;
    right_arm.1.x += (age * 0.067).sin() * 0.05;
    left_arm.1.x -= (age * 0.067).sin() * 0.05;
    [head, body, right_arm, left_arm, right_leg, left_leg]
}

/// Places every player model and name tag for this frame.
#[allow(clippy::too_many_arguments)]
pub fn pose_others(
    clock: Res<Clock>,
    view: Res<View>,
    hide: Res<HideGui>,
    streamer: Res<Streamer>,
    plate_mats: Option<Res<PlateMaterials>>,
    mut materials: ResMut<Assets<EntityMaterial>>,
    players: Query<(Entity, &Other, Has<OwnModel>)>,
    mut transforms: Query<&mut Transform>,
    mut visibility: Query<&mut Visibility>,
    mut plate_handles: Query<&mut MeshMaterial3d<NameplateMaterial>>,
) {
    let partial = clock.partial;
    let set_vis = |visibility: &mut Query<&mut Visibility>, e: Entity, on: bool| {
        if let Ok(mut v) = visibility.get_mut(e) {
            let want = if on { Visibility::Inherited } else { Visibility::Hidden };
            if *v != want {
                *v = want;
            }
        }
    };
    // RenderManager.playerViewY and playerViewX, turned round for the front view.
    let (view_yaw, view_pitch) = if view.third_person == 2 { (view.yaw + 180.0, -view.pitch) } else { (view.yaw, view.pitch) };
    for (e, o, own) in &players {
        let shown = o.increments >= 0 && (!own || view.third_person != 0);
        set_vis(&mut visibility, e, shown);
        if !shown {
            if let Some(p) = o.plate {
                for piece in [p.bg, p.through, p.text] {
                    set_vis(&mut visibility, piece, false);
                }
            }
            continue;
        }
        let mut pos = o.prev_pos.lerp(o.pos, partial as f64);
        if o.sneaking() {
            // RenderPlayer.doRender
            pos.y -= 0.125;
        }
        let body_yaw = lerp(o.prev_body_yaw, o.body_yaw, partial);
        // RenderLivingBase.doRender: rotate 180 - renderYawOffset, scale (-1, -1, 1), the
        // player's 0.9375, translate down 1.501, then the model's 1/16 units.
        let root = Mat4::from_translation(pos.as_vec3())
            * Mat4::from_rotation_y((180.0 - body_yaw).to_radians())
            * Mat4::from_scale(Vec3::new(-0.9375, -0.9375, 0.9375))
            * Mat4::from_translation(Vec3::new(0.0, -1.501, 0.0))
            * Mat4::from_scale(Vec3::splat(1.0 / 16.0));
        if let Ok(mut t) = transforms.get_mut(e) {
            *t = Transform::from_matrix(root);
        }
        for (i, (pivot, angles)) in pose(o, partial).into_iter().enumerate() {
            if let Ok(mut t) = transforms.get_mut(o.parts[i]) {
                // ModelRenderer.render: translate to the pivot, rotate Z, Y, X.
                *t = Transform { translation: pivot, rotation: Quat::from_euler(EulerRot::ZYX, angles.z, angles.y, angles.x), scale: Vec3::ONE };
            }
        }
        // getBrightnessForRender at the eyes: the lightmap coordinates the shader looks up.
        let eye = pos + DVec3::new(0.0, 1.62, 0.0);
        let (sky, block) = neighbor_light(&streamer, eye.x.floor() as i32, eye.y.floor() as i32, eye.z.floor() as i32);
        let light = Vec4::new(block as f32 * 16.0, sky as f32 * 16.0, 0.0, 0.0);
        if materials.get(&o.material).is_some_and(|m| m.light != light) {
            if let Some(mut m) = materials.get_mut(&o.material) {
                m.light = light;
            }
        }

        // Render.renderLivingLabel: within 64 blocks of the viewer (32 sneaking), with the GUI.
        let Some(p) = o.plate else { continue };
        let Some(pm) = plate_mats.as_deref() else { continue };
        let sneak = o.sneaking();
        let range = if sneak { 32.0 } else { 64.0 };
        let near = (o.pos - view.feet.as_dvec3()).length_squared() <= range * range;
        let on = near && !hide.0;
        set_vis(&mut visibility, p.bg, on);
        set_vis(&mut visibility, p.through, on && !sneak);
        set_vis(&mut visibility, p.text, on);
        if !on {
            continue;
        }
        // drawNameplate: over the head by height + 0.5 (0.25 less sneaking), turned to the
        // viewer, scaled to 0.025 a font pixel with y down.
        let at = pos.as_vec3() + Vec3::Y * (1.8 + 0.5 - if sneak { 0.25 } else { 0.0 });
        let m = Mat4::from_translation(at)
            * Mat4::from_rotation_y((-view_yaw).to_radians())
            * Mat4::from_rotation_x(view_pitch.to_radians())
            * Mat4::from_scale(Vec3::new(-0.025, -0.025, 0.025));
        // The background sorts behind the text in the blended pass: it draws first, as vanilla
        // draws it first.
        let away = (at - view.eye).normalize_or_zero() * 0.05;
        for (piece, offset) in [(p.bg, away), (p.through, Vec3::ZERO), (p.text, Vec3::ZERO)] {
            if let Ok(mut t) = transforms.get_mut(piece) {
                *t = Transform::from_matrix(Mat4::from_translation(offset) * m);
            }
        }
        for (piece, want) in [(p.bg, if sneak { &pm.bg_depth } else { &pm.bg }), (p.text, if sneak { &pm.text_dim } else { &pm.text })] {
            if let Ok(mut h) = plate_handles.get_mut(piece) {
                if h.0 != *want {
                    h.0 = want.clone();
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn other() -> Other {
        Other::new(None, [Entity::PLACEHOLDER; 6], Handle::default())
    }

    #[test]
    fn moves_to_the_target_in_three_ticks() {
        let mut o = other();
        o.set_target(PlayerState { pos: [0.0, 64.0, 0.0], ..default() });
        assert_eq!(o.pos, DVec3::new(0.0, 64.0, 0.0));
        o.set_target(PlayerState { pos: [3.0, 64.0, 0.0], yaw: 90.0, ..default() });
        o.tick();
        assert!((o.pos.x - 1.0).abs() < 1e-9);
        o.tick();
        o.tick();
        assert!((o.pos.x - 3.0).abs() < 1e-9 && (o.yaw - 90.0).abs() < 1e-4);
        assert!(o.limb_amount > 0.5, "walking swings the limbs");
    }

    #[test]
    fn font_widths_follow_the_last_opaque_column() {
        // A 128 x 128 sheet: 'A' (65) filled in its first 5 columns, '!' (33) in its first one.
        let mut px = vec![0u32; 128 * 128];
        for (ch, cols) in [(65usize, 5usize), (33, 1)] {
            let (gx, gy) = ((ch % 16) * 8, (ch / 16) * 8);
            for y in 0..7 {
                for x in 0..cols {
                    px[(gy + y) * 128 + gx + x] = 0xFF00_0000;
                }
            }
        }
        let w = Font::widths(&Argb { width: 128, height: 128, pixels: px });
        assert_eq!((w[65], w[33], w[32], w[66]), (6, 2, 4, 1));
    }

    #[test]
    fn own_model_follows_the_player() {
        let mut o = other();
        o.follow(PlayerState { pos: [1.0, 64.0, 2.0], yaw: 30.0, ..default() });
        assert_eq!(o.pos, DVec3::new(1.0, 64.0, 2.0));
        o.follow(PlayerState { pos: [1.2, 64.0, 2.0], yaw: 30.0, ..default() });
        assert_eq!((o.prev_pos.x, o.pos.x), (1.0, 1.2));
        assert!(o.limb_amount > 0.0, "walking swings the limbs");
    }

    #[test]
    fn steps_come_every_five_thirds_of_a_block_on_the_ground() {
        let mut o = other();
        o.target.flags = proto::ON_GROUND;
        let mut steps = 0;
        // 0.25 blocks a tick for 39 ticks: 9.75 blocks, 5.85 walked units, a step past each whole one.
        for _ in 0..39 {
            steps += step_due(&mut o, DVec3::new(0.25, 0.0, 0.0)) as u32;
        }
        assert_eq!(steps, 5);
        o.target.flags = proto::FLYING;
        assert!(!step_due(&mut o, DVec3::new(5.0, 0.0, 0.0)), "flying is silent");
    }

    #[test]
    fn a_new_swing_count_starts_a_swing() {
        let mut o = other();
        o.set_target(PlayerState { swings: 4, ..default() });
        assert!(!o.swinging);
        o.set_target(PlayerState { swings: 5, ..default() });
        assert!(o.swinging);
        // swingProgressInt runs from -1 up to SWING_END.
        for _ in 0..6 {
            o.tick();
        }
        assert!(o.swinging);
        o.tick();
        assert!(!o.swinging);
    }

    #[test]
    fn body_follows_the_head_within_75_degrees() {
        let mut o = other();
        o.set_target(PlayerState { yaw: 0.0, ..default() });
        o.set_target(PlayerState { yaw: 120.0, ..default() });
        for _ in 0..20 {
            o.tick();
        }
        assert!(wrap(o.yaw - o.body_yaw).abs() <= 75.0);
    }
}
