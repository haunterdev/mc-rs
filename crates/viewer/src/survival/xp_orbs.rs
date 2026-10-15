//! Experience orbs in a server session: the `S_SPAWN_OBJECT` orbs (`ObjectData::XpOrb`), flown on
//! the client between the server's `S_ENTITY_MOVE` updates as `EntityXPOrb.onUpdate` moves them
//! (`sim::xp_orb`, the same tick the server runs), and drawn as `RenderXPOrb.doRender` draws them.
//!
//! `S_COLLECT` for an orb is `handleCollectItem` for an `EntityXPOrb`: the entity is removed and
//! `ENTITY_EXPERIENCE_ORB_PICKUP` plays (`PLAYERS`, volume 0.1, pitch `0.5 * ((r - r) * 0.7 + 1.8)`).
//!
//! The draw is one camera-facing quad per orb on `textures/entity/experience_orb.png` (a 64 by 64
//! sheet of 16 by 16 icons, the frame from `getTextureByXP`), tinted by the pulsing green of
//! `RenderXPOrb`, multiplied by the lightmap at the orb (`getBrightnessForRender`), scaled 0.3.
//!
//! DROPPED: the shadow, and the lava burn sounds (the server's).

use std::collections::HashMap;

use bevy::asset::RenderAssetUsages;
use bevy::camera::visibility::NoFrustumCulling;
use bevy::light::NotShadowCaster;
use bevy::math::DVec3;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use proto::{Collect, EntityMove, ObjectData, SpawnObject, Velocity};
use sim::xp_orb::{Target, XpOrb};
use worldgen::JavaRandom;

use super::{Collect as CollectMsg, Destroy, EntityMoves, SpawnObject as SpawnMsg, Velocity as VelocityMsg};
use crate::audio::{Audio, Category};
use crate::collision::{floor, BlockAccess};
use crate::player::Player;
use crate::env::Env;
use crate::sky::{entity_light, Clock, View};
use crate::stream::Streamer;

/// `textures/entity/experience_orb.png`, loaded at startup (main.rs).
#[derive(Resource)]
pub struct XpOrbImage(pub Handle<Image>);

/// The frames of the sheet (`getTextureByXP` is 0 to 10).
const FRAMES: usize = 11;

/// `RenderXPOrb.doRender`'s texture window: the four uv corners `(u, v)` of frame `i`, 16 pixels
/// square at `(i % 4 * 16, i / 4 * 16)` on the 64 pixel sheet. Corners in the order of
/// [`quad_corners`].
pub fn frame_uvs(i: i32) -> [[f32; 2]; 4] {
    let f = (i % 4 * 16) as f32 / 64.0;
    let f1 = (i % 4 * 16 + 16) as f32 / 64.0;
    let f2 = (i / 4 * 16) as f32 / 64.0;
    let f3 = (i / 4 * 16 + 16) as f32 / 64.0;
    [[f, f3], [f1, f3], [f1, f2], [f, f2]]
}

/// The quad `(-0.5, -0.25)` to `(0.5, 0.75)` of `doRender`.
pub fn quad_corners() -> [Vec3; 4] {
    [Vec3::new(-0.5, -0.25, 0.0), Vec3::new(0.5, -0.25, 0.0), Vec3::new(0.5, 0.75, 0.0), Vec3::new(-0.5, 0.75, 0.0)]
}

/// The vertex colour: `(sin(f) + 1) * 0.5 * 255`, 255, `(sin(f + 4.1887903) + 1) * 0.1 * 255` with
/// `f = (xpColor + partialTicks) / 2`, each truncated to an integer, as 0 to 255.
pub fn tint(color: i32, partial: f32) -> [u8; 3] {
    let f = (color as f32 + partial) / 2.0;
    let r = ((worldgen::math_helper::sin(f) + 1.0) * 0.5 * 255.0) as i32;
    let b = ((worldgen::math_helper::sin(f + 4.188_790_3) + 1.0) * 0.1 * 255.0) as i32;
    [r as u8, 255, b as u8]
}

/// `RenderXPOrb.doRender`'s matrix: translate to the feet plus 0.1, `rotate(180 - playerViewY, y)`,
/// `rotate(-playerViewX, x)`, scale 0.3.
pub fn billboard(at: Vec3, view_yaw: f32, view_pitch: f32) -> Mat4 {
    Mat4::from_translation(at + Vec3::new(0.0, 0.1, 0.0))
        * Mat4::from_rotation_y((180.0 - view_yaw).to_radians())
        * Mat4::from_rotation_x((-view_pitch).to_radians())
        * Mat4::from_scale(Vec3::splat(0.3))
}

/// One orb as this viewer knows it.
#[derive(Clone, Debug)]
pub struct Client {
    pub orb: XpOrb,
}

impl Client {
    /// The interpolated feet, `lastTickPos` to `pos`.
    pub fn feet(&self, partial: f32) -> DVec3 {
        let b = &self.orb.body;
        b.prev_pos + (b.pos - b.prev_pos) * partial as f64
    }
}

/// The orbs of the session.
#[derive(Resource)]
pub struct XpOrbs {
    pub map: HashMap<u32, Client>,
    rand: JavaRandom,
    /// Entity and material per orb id, for the draw.
    drawn: HashMap<u32, (Entity, Handle<StandardMaterial>)>,
    /// Where `S_COLLECT` took an orb this frame, for `collect_effects` to sound.
    collected: Vec<DVec3>,
}

impl Default for XpOrbs {
    fn default() -> XpOrbs {
        XpOrbs { map: HashMap::new(), rand: JavaRandom::new(0x58_50_4F_52), drawn: HashMap::new(), collected: Vec::new() }
    }
}

impl XpOrbs {
    /// `handleSpawnExperienceOrb`: `new EntityXPOrb(world, x, y, z, value)`. The motion is the
    /// packet's here (vanilla's packet has none and the orb starts still).
    pub fn spawn(&mut self, m: &SpawnObject) {
        let ObjectData::XpOrb { value } = m.data else { return };
        let seed = self.rand.next_long();
        let mut orb = XpOrb::new(m.pos[0], m.pos[1], m.pos[2], value as i32, JavaRandom::new(seed));
        orb.body.motion = DVec3::new(m.motion[0] as f64, m.motion[1] as f64, m.motion[2] as f64);
        self.map.insert(m.id, Client { orb });
    }

    /// `handleEntityTeleport` for an `Entity`: position, `onGround`.
    pub fn moves(&mut self, moves: &[EntityMove]) {
        for m in moves {
            if let Some(c) = self.map.get_mut(&m.id) {
                c.orb.body.set_position(DVec3::new(m.pos[0], m.pos[1], m.pos[2]));
                c.orb.body.on_ground = m.on_ground;
            }
        }
    }

    /// `handleEntityVelocity`: `Entity.setVelocity`.
    pub fn velocity(&mut self, v: &Velocity) {
        if let Some(c) = self.map.get_mut(&v.id) {
            c.orb.body.motion = DVec3::new(v.motion[0] as f64, v.motion[1] as f64, v.motion[2] as f64);
        }
    }

    /// `handleDestroyEntities`
    pub fn destroy(&mut self, ids: &[u32]) {
        for id in ids {
            self.map.remove(id);
        }
    }

    /// `handleCollectItem` for an orb: the entity leaves the world. Returns where it was, for the
    /// pickup sound.
    pub fn collect(&mut self, c: &Collect) -> Option<DVec3> {
        let orb = self.map.remove(&c.item)?;
        Some(orb.orb.body.pos)
    }

    /// One client tick: `EntityXPOrb.onUpdate` for every orb in a loaded chunk, steering toward
    /// the given players.
    pub fn tick(&mut self, w: &impl BlockAccess, targets: &[Target]) {
        for (&id, c) in self.map.iter_mut() {
            let p = c.orb.body.pos;
            if !w.loaded(floor(p.x), floor(p.z)) {
                continue;
            }
            c.orb.tick(id, w, targets, true);
            // The burn sound is the server's (`Entity.playSound` is silent on a client).
            c.orb.sounds.clear();
            c.orb.body.sounds.clear();
        }
    }
}

/// The pickup sound of `handleCollectItem`: `PLAYERS`, volume 0.1, pitch `0.5 * ((r - r) * 0.7 + 1.8)`.
pub fn pickup_pitch(a: f32, b: f32) -> f32 {
    0.5 * ((a - b) * 0.7 + 1.8)
}

fn read_frames(
    mut orbs: ResMut<XpOrbs>,
    mut spawns: MessageReader<SpawnMsg>,
    mut moves: MessageReader<EntityMoves>,
    mut velocity: MessageReader<VelocityMsg>,
    mut collects: MessageReader<CollectMsg>,
    mut destroy: MessageReader<Destroy>,
) {
    for SpawnMsg(m) in spawns.read() {
        orbs.spawn(m);
    }
    for EntityMoves(m) in moves.read() {
        orbs.moves(m);
    }
    for VelocityMsg(v) in velocity.read() {
        orbs.velocity(v);
    }
    // S_COLLECT before S_DESTROY, as the server sends them.
    for CollectMsg(c) in collects.read() {
        if let Some(at) = orbs.collect(c) {
            orbs.collected.push(at);
        }
    }
    for Destroy(ids) in destroy.read() {
        orbs.destroy(ids);
    }
}

fn collect_effects(mut orbs: ResMut<XpOrbs>, audio: Option<Res<Audio>>) {
    for at in std::mem::take(&mut orbs.collected) {
        if let Some(audio) = &audio {
            let pitch = pickup_pitch(audio.next_float(), audio.next_float());
            audio.play("entity.experience_orb.pickup", Category::Players, Some(at.as_vec3()), 0.1, pitch);
        }
    }
}

fn tick(clock: Res<Clock>, streamer: Res<Streamer>, player: Res<Player>, mut orbs: ResMut<XpOrbs>) {
    let p = player.body.pos;
    // The session player only: a stand-in id that no entity has.
    let targets = [Target { id: u32::MAX, pos: [p.x, p.y, p.z], eye_height: 1.62, spectator: false }];
    for _ in 0..clock.ticks_this_frame {
        orbs.tick(&*streamer, &targets);
    }
}

fn frame_mesh(frame: i32) -> Mesh {
    let pos: Vec<[f32; 3]> = quad_corners().iter().map(|c| c.to_array()).collect();
    Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, pos)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.0, 0.0, 1.0]; 4])
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, frame_uvs(frame).to_vec())
        .with_inserted_indices(Indices::U32(vec![0, 1, 2, 0, 2, 3]))
}

/// The orb color times the lightmap color at the orb.
fn lit(rgb: [u8; 3], l: [f32; 3]) -> Color {
    Color::srgb(rgb[0] as f32 / 255.0 * l[0], rgb[1] as f32 / 255.0 * l[1], rgb[2] as f32 / 255.0 * l[2])
}

fn material(image: &XpOrbImage, color: Color) -> StandardMaterial {
    StandardMaterial {
        base_color: color,
        base_color_texture: Some(image.0.clone()),
        unlit: true,
        double_sided: true,
        cull_mode: None,
        fog_enabled: false,
        alpha_mode: AlphaMode::Mask(0.1),
        ..default()
    }
}

#[allow(clippy::too_many_arguments)]
fn draw(
    mut commands: Commands,
    clock: Res<Clock>,
    view: Res<View>,
    env: Res<Env>,
    streamer: Res<Streamer>,
    image: Option<Res<XpOrbImage>>,
    mut orbs: ResMut<XpOrbs>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut frames: Local<Vec<Handle<Mesh>>>,
    mut shown: Query<&mut Transform>,
) {
    let Some(image) = image else { return };
    if frames.is_empty() {
        *frames = (0..FRAMES as i32).map(|i| meshes.add(frame_mesh(i))).collect();
    }
    let partial = clock.partial;
    let eye = view.eye.as_dvec3();
    let orbs = &mut *orbs;
    let gone: Vec<u32> = orbs.drawn.keys().filter(|id| !orbs.map.contains_key(id)).copied().collect();
    for id in gone {
        if let Some((e, m)) = orbs.drawn.remove(&id) {
            commands.entity(e).despawn();
            materials.remove(&m);
        }
    }
    for (id, c) in &orbs.map {
        let feet = c.feet(partial);
        // Entity.isInRangeToRenderDist: 64 * the box's average edge (0.5), scaled.
        if feet.distance_squared(eye) > 64.0 * 64.0 {
            continue;
        }
        let m = billboard(feet.as_vec3(), view.yaw, view.pitch);
        let rgb = tint(c.orb.color, partial);
        let color = lit(rgb, entity_light(&env, &streamer, c.orb.body.pos));
        match orbs.drawn.get(id) {
            Some((e, mat)) => {
                if let Ok(mut t) = shown.get_mut(*e) {
                    *t = Transform::from_matrix(m);
                }
                if let Some(mut mat) = materials.get_mut(mat) {
                    mat.base_color = color;
                }
            }
            None => {
                let mat = materials.add(material(&image, color));
                let frame = c.orb.texture_by_xp().clamp(0, FRAMES as i32 - 1) as usize;
                let e = commands
                    .spawn((Mesh3d(frames[frame].clone()), MeshMaterial3d(mat.clone()), Transform::from_matrix(m), Visibility::Inherited, NoFrustumCulling, NotShadowCaster))
                    .id();
                orbs.drawn.insert(*id, (e, mat));
            }
        }
    }
}

pub fn plugin(app: &mut App) {
    app.init_resource::<XpOrbs>().add_systems(
        Update,
        (
            read_frames,
            collect_effects.after(read_frames),
            tick.after(crate::player::run_ticks).after(read_frames).run_if(resource_exists::<Streamer>).run_if(resource_exists::<Clock>).run_if(resource_exists::<Player>),
            draw.after(crate::sky::advance_clock).after(read_frames).after(tick).run_if(resource_exists::<Streamer>).run_if(resource_exists::<Clock>).run_if(resource_exists::<View>).run_if(resource_exists::<Env>),
        ),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::collision::tests::Blocks;

    fn spawn_msg(id: u32, pos: [f64; 3], value: u16) -> SpawnObject {
        SpawnObject { id, pos, motion: [0.0, 0.2, 0.0], data: ObjectData::XpOrb { value }, ..Default::default() }
    }

    #[test]
    fn frames_move_an_orb_and_a_collect_removes_it() {
        let mut orbs = XpOrbs::default();
        let w = Blocks::floor_at(63, 8);
        orbs.spawn(&spawn_msg(7, [0.5, 66.0, 0.5], 5));
        // Something that is not an orb is ignored.
        orbs.spawn(&SpawnObject { id: 8, data: ObjectData::Arrow { shooter: 1 }, ..Default::default() });
        assert_eq!(orbs.map.len(), 1);
        assert_eq!(orbs.map[&7].orb.value, 5);
        assert_eq!(orbs.map[&7].orb.body.motion.y, 0.2f32 as f64);
        // It falls onto the stone and rests on top.
        for _ in 0..80 {
            orbs.tick(&w, &[]);
        }
        let c = &orbs.map[&7];
        assert!(c.orb.body.on_ground && (c.orb.body.pos.y - 64.0).abs() < 1.0e-3, "{:?}", c.orb.body.pos);
        orbs.moves(&[EntityMove { id: 7, pos: [2.0, 65.0, 2.0], on_ground: false, ..Default::default() }]);
        assert_eq!(orbs.map[&7].orb.body.pos, DVec3::new(2.0, 65.0, 2.0));
        orbs.velocity(&Velocity { id: 7, motion: [0.1, 0.0, 0.0] });
        assert_eq!(orbs.map[&7].orb.body.motion.x, 0.1f32 as f64);
        let at = orbs.collect(&Collect { item: 7, collector: 1, count: 1 }).unwrap();
        assert_eq!(at, DVec3::new(2.0, 65.0, 2.0));
        assert!(orbs.map.is_empty());
        assert!(orbs.collect(&Collect { item: 7, collector: 1, count: 1 }).is_none());
        orbs.spawn(&spawn_msg(9, [0.5, 66.0, 0.5], 1));
        orbs.destroy(&[9]);
        assert!(orbs.map.is_empty());
    }

    #[test]
    fn an_orb_in_an_unloaded_chunk_waits() {
        struct Unloaded;
        impl BlockAccess for Unloaded {
            fn state(&self, _: i32, _: i32, _: i32) -> u16 {
                0
            }
            fn loaded(&self, _: i32, _: i32) -> bool {
                false
            }
        }
        let mut orbs = XpOrbs::default();
        orbs.spawn(&spawn_msg(1, [0.5, 66.0, 0.5], 3));
        orbs.tick(&Unloaded, &[]);
        assert_eq!(orbs.map[&1].orb.age, 0);
    }

    #[test]
    fn the_value_picks_the_frame_and_the_uv_window() {
        // getTextureByXP: 1-2 is 0, 3-6 is 1, ... 2477+ is 10.
        let frame = |v: i32| XpOrb::new(0.0, 0.0, 0.0, v, JavaRandom::new(1)).texture_by_xp();
        assert_eq!((frame(1), frame(3), frame(7), frame(2477)), (0, 1, 2, 10));
        // Frame 0 is the top left icon of the 64 pixel sheet, frame 5 is one right and one down,
        // frame 10 is the third column of the third row.
        assert_eq!(frame_uvs(0), [[0.0, 0.25], [0.25, 0.25], [0.25, 0.0], [0.0, 0.0]]);
        assert_eq!(frame_uvs(5), [[0.25, 0.5], [0.5, 0.5], [0.5, 0.25], [0.25, 0.25]]);
        assert_eq!(frame_uvs(10), [[0.5, 0.75], [0.75, 0.75], [0.75, 0.5], [0.5, 0.5]]);
    }

    #[test]
    fn the_tint_pulses_green_and_the_pickup_pitch_is_the_vanilla_one() {
        // f = 0: r = (0 + 1) * 0.5 * 255 = 127, b = (sin(4.1887903) + 1) * 0.1 * 255 = 3.
        assert_eq!(tint(0, 0.0), [127, 255, 3]);
        // Every colour keeps green full and red within the range.
        for c in 0..200 {
            let t = tint(c, 0.5);
            assert_eq!(t[1], 255);
            assert!(t[2] <= 51);
        }
        assert!((pickup_pitch(0.5, 0.5) - 0.9).abs() < 1e-6);
        assert!((pickup_pitch(1.0, 0.0) - 1.25).abs() < 1e-6);
    }

    #[test]
    fn the_billboard_faces_the_camera_at_the_scale_of_zero_point_three() {
        let m = billboard(Vec3::new(1.0, 2.0, 3.0), 0.0, 0.0);
        assert!((m.transform_point3(Vec3::ZERO) - Vec3::new(1.0, 2.1, 3.0)).length() < 1e-5);
        // The quad is 1 wide at scale 0.3.
        let w = m.transform_point3(Vec3::new(0.5, 0.0, 0.0)) - m.transform_point3(Vec3::new(-0.5, 0.0, 0.0));
        assert!((w.length() - 0.3).abs() < 1e-5);
        // yaw 0 turns the quad half way round (180 - 0), so its front looks toward -z... the camera
        // at yaw 0 looks along +z, so the quad's normal points back at it.
        let n = m.transform_vector3(Vec3::Z).normalize();
        assert!((n - Vec3::NEG_Z).length() < 1e-5, "{n:?}");
    }
}
