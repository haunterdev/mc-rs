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
//! draws too, following the player each tick. What each model holds is `held.rs`, what it wears
//! `armor.rs`.

mod armor;
mod held;
mod plate;
mod posing;

pub use armor::{texture_path as armor_texture_path, update_armor, ArmorImages, MATERIALS as ARMOR_MATERIALS};
pub use held::{held_fields, local_matrix, pose_held};
pub use plate::{Font, PlateMaterials};
use plate::{plate_meshes, Plate};
pub use posing::pose_others;
use posing::{body_matrix, pose};

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

impl Others {
    /// The model entity of the player with this id.
    pub fn entity_of(&self, id: u32) -> Option<Entity> {
        self.by_id.get(&id).copied()
    }

    /// Every other player's id and model entity.
    pub fn all(&self) -> impl Iterator<Item = (u32, Entity)> + '_ {
        self.by_id.iter().map(|(&id, &e)| (id, e))
    }
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

/// Marks the model of the inventory screens (`GuiInventory.drawEntityOnScreen`): a second copy of
/// the player's model on [`PREVIEW_LAYER`], seen by [`InventoryCamera`] only.
#[derive(Component)]
pub struct InventoryModel;

/// The orthographic camera that draws the [`InventoryModel`] over the screen (order 3).
#[derive(Component)]
pub struct InventoryCamera;

/// The render layer of the inventory model and its camera.
pub const PREVIEW_LAYER: usize = 4;

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
    /// `Entity.getEntityBoundingBox` for the player's `setSize(0.6, 1.8)` (1.65 high sneaking),
    /// centred on the feet, or None before the first position arrived.
    pub fn bb(&self) -> Option<sim::collision::Aabb> {
        if self.pos == DVec3::ZERO {
            return None;
        }
        let h = if self.target.flags & proto::SNEAKING != 0 { 1.65 } else { 1.8 };
        let f = 0.3f32 as f64;
        Some(sim::collision::Aabb::new(self.pos.x - f, self.pos.y, self.pos.z - f, self.pos.x + f, self.pos.y + h, self.pos.z + f))
    }

    /// The feet between the last two ticks (`prevPos` to `pos` at `partial`).
    pub fn interp_feet(&self, partial: f32) -> DVec3 {
        self.prev_pos + (self.pos - self.prev_pos) * partial as f64
    }

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
    let material = materials.add(EntityMaterial::new(skin.env.clone(), skin.skin.clone()));
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
    let (held, held_item, held_meta) = held_fields(&inv.get_current_item());
    let (_, held_item_off, held_meta_off) = held_fields(&inv.get_stack_in_slot(40));
    o.follow(PlayerState { pos: player.pos.to_array(), yaw: player.yaw, pitch: player.pitch, flags, swings: hand.swings, held, held_item, held_meta, held_item_off, held_meta_off });
}

/// `GuiInventory.drawEntityOnScreen`: while a screen asks for the player model, a copy of the
/// player's model stands at the player's feet (so `getBrightnessForRender` lights it as the world
/// does), turned toward the pointer as `drawEntityOnScreen` sets `renderYawOffset`, `rotationYaw`
/// and `rotationPitch`, and the camera puts its feet at the GUI pixel the screen names, `scale`
/// pixels per block. Worn armor and the held item come from the same systems as every model.
/// DROPPED: the real player's limb swing and arm swing (the model stands still), the shadow.
#[allow(clippy::too_many_arguments)]
pub fn inventory_model(
    mut commands: Commands,
    clock: Res<Clock>,
    player: Res<Player>,
    inv: Res<Inventory>,
    skin: Res<Skin>,
    font: Res<Font>,
    frame: Option<Res<crate::survival::gui::draw::Frame>>,
    window: Single<&Window, With<bevy::window::PrimaryWindow>>,
    have: Option<(Res<PartMeshes>, Res<PlateMaterials>)>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<EntityMaterial>>,
    mut plates: ResMut<Assets<NameplateMaterial>>,
    mut model: Query<&mut Other, With<InventoryModel>>,
    mut camera: Query<(&mut Camera, &mut Transform, &mut Projection), With<InventoryCamera>>,
) {
    use bevy::camera::visibility::RenderLayers;
    let want = frame.as_deref().and_then(|f| f.player_model).filter(|_| player.spawned);
    let Ok(mut o) = model.single_mut() else {
        if want.is_some() {
            let (parts, plate_mats) = shared(&mut commands, &mut meshes, &mut plates, &font, have.as_ref().map(|(a, b)| (&**a, &**b)));
            let root = spawn_model(&mut commands, &mut meshes, &mut materials, &skin, &font, &parts, &plate_mats, None);
            commands.entity(root).insert((InventoryModel, RenderLayers::layer(PREVIEW_LAYER)));
            commands.spawn((
                InventoryCamera,
                Camera3d::default(),
                Camera { order: 3, is_active: false, clear_color: ClearColorConfig::None, ..default() },
                Projection::Orthographic(OrthographicProjection {
                    near: 0.1,
                    far: 100.0,
                    scaling_mode: bevy::camera::ScalingMode::Fixed { width: 1.0, height: 1.0 },
                    ..OrthographicProjection::default_3d()
                }),
                bevy::core_pipeline::tonemapping::Tonemapping::None,
                bevy::core_pipeline::tonemapping::DebandDither::Disabled,
                Msaa::Off,
                Transform::default(),
                RenderLayers::layer(PREVIEW_LAYER),
            ));
        }
        return;
    };
    let Some(spec) = want else {
        // No screen asks: hidden (pose_others shows only increments >= 0), and the camera off.
        o.increments = -1;
        for (mut c, ..) in &mut camera {
            c.is_active = false;
        }
        return;
    };
    let mut flags = 0;
    if player.is_sneaking() {
        flags |= proto::SNEAKING;
    }
    let (held, held_item, held_meta) = held_fields(&inv.get_current_item());
    let (_, held_item_off, held_meta_off) = held_fields(&inv.get_stack_in_slot(40));
    o.set_target(PlayerState { pos: player.pos.to_array(), yaw: player.yaw, pitch: player.pitch, flags, held, held_item, held_meta, held_item_off, held_meta_off, ..default() });
    o.age += clock.ticks_this_frame;
    // drawEntityOnScreen: renderYawOffset atan(x / 40) * 20, rotationYaw atan(x / 40) * 40,
    // rotationPitch -atan(y / 40) * 20 (the atan in radians, the product in degrees); the
    // previous values the same (partial tick 1).
    let (f, g) = ((spec.mouse_x / 40.0).atan(), (spec.mouse_y / 40.0).atan());
    o.increments = 0;
    o.pos = DVec3::from_array(player.pos.to_array());
    o.prev_pos = o.pos;
    (o.body_yaw, o.prev_body_yaw) = (f * 20.0, f * 20.0);
    (o.yaw, o.prev_yaw) = (f * 40.0, f * 40.0);
    (o.pitch, o.prev_pitch) = (-g * 20.0, -g * 20.0);
    (o.limb_swing, o.limb_amount, o.prev_limb_amount, o.swing, o.prev_swing, o.swing_int, o.swinging) = (0.0, 0.0, 0.0, 0.0, 0.0, 0, false);

    // The camera: the feet at GUI pixel (x, y) with `scale` pixels per block, looking down -z at
    // a model that faces +z.
    let (w, h) = (window.physical_width(), window.physical_height());
    let k = crate::hand::gui_scale(w, h) as f32;
    let (gw, gh) = (w as f32 / k, h as f32 / k);
    let s = spec.scale as f32;
    let feet = o.pos.as_vec3();
    for (mut c, mut t, mut p) in &mut camera {
        c.is_active = true;
        if let Projection::Orthographic(op) = &mut *p {
            op.scaling_mode = bevy::camera::ScalingMode::Fixed { width: gw / s, height: gh / s };
        }
        t.translation = feet + Vec3::new(-(spec.x as f32 - gw / 2.0) / s, -(gh / 2.0 - spec.y as f32) / s, 10.0);
    }
}

/// After the model is posed: `rotate(-atan(mouseY / 40) * 20, 1, 0, 0)` about the feet, and the
/// render layer on every part, armor box and held item below the inventory model's root.
pub fn finish_inventory_model(
    mut commands: Commands,
    frame: Option<Res<crate::survival::gui::draw::Frame>>,
    mut root: Query<(Entity, &Other, &mut Transform), With<InventoryModel>>,
    children: Query<&Children>,
    layers: Query<&bevy::camera::visibility::RenderLayers>,
) {
    let Ok((e, o, mut t)) = root.single_mut() else { return };
    for d in children.iter_descendants(e) {
        if layers.get(d).is_err() {
            commands.entity(d).insert(bevy::camera::visibility::RenderLayers::layer(PREVIEW_LAYER));
        }
    }
    let Some(spec) = frame.as_deref().and_then(|f| f.player_model) else { return };
    let a = -(spec.mouse_y / 40.0).atan() * 20.0;
    let feet = o.pos.as_vec3();
    let tilt = Mat4::from_translation(feet) * Mat4::from_rotation_x(a.to_radians()) * Mat4::from_translation(-feet);
    *t = Transform::from_matrix(tilt * t.to_matrix());
}

/// This frame's client ticks for every other player, with their footsteps.
pub fn tick_others(clock: Res<Clock>, streamer: Res<Streamer>, audio: Res<crate::audio::Audio>, mut players: Query<&mut Other, (Without<OwnModel>, Without<InventoryModel>)>) {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::textures::Argb;

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
