//! Arrows in a server session: the `S_SPAWN_OBJECT` arrows (`ObjectData::Arrow`), flown on the
//! client between the server's `S_ENTITY_MOVE` updates as `EntityArrow.onUpdate` flies them
//! (gravity 0.05, drag 0.99, 0.6 in water), and drawn as `RenderArrow.doRender` draws them.
//!
//! `S_ENTITY_MOVE` is `handleEntityTeleport` (`setPositionAndRotationDirect`, `onGround`): an arrow
//! the server reports on the ground is `inGround` and stops moving. `S_VELOCITY` is
//! `EntityArrow.setVelocity`, `S_DESTROY` removes it.
//!
//! The draw is one mesh of the crossed quads and the back cross (`RenderArrow`), unlit as
//! `disableLighting` but multiplied by the lightmap at the arrow (`Entity.getBrightnessForRender`,
//! `RenderManager.renderEntity`), on `textures/entity/projectiles/arrow.png`, one entity and
//! one material (the light tint) per arrow.
//!
//! DROPPED: the block and entity ray trace of `onUpdate` (the arrow stops where its position
//! enters a collision box instead, and the server's move frame is the truth), the crit and bubble
//! particles, `arrowShake` (the shake rotate of `doRender`), `ticksInGround` despawn (the server
//! destroys), the `inTile` check that frees an arrow when its block is gone, `doBlockCollisions`,
//! the tipped arrow colour, the outline mode and the shadow.

use std::collections::HashMap;

use bevy::asset::RenderAssetUsages;
use bevy::camera::visibility::NoFrustumCulling;
use bevy::light::NotShadowCaster;
use bevy::math::DVec3;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use proto::{EntityMove, ObjectData, SpawnObject, Velocity};

use super::{Destroy, EntityMoves, SpawnObject as SpawnMsg, Velocity as VelocityMsg};
use crate::collision::{floor, get_collision_boxes, handle_material_acceleration, Aabb, BlockAccess};
use crate::region::Mat;
use crate::env::Env;
use crate::sky::{entity_light, Clock, View};
use crate::stream::Streamer;

/// `textures/entity/projectiles/arrow.png`, loaded at startup (main.rs).
#[derive(Resource)]
pub struct ArrowImage(pub Handle<Image>);

/// One arrow as this viewer knows it.
#[derive(Clone, Debug, PartialEq)]
pub struct Arrow {
    pub pos: DVec3,
    pub prev_pos: DVec3,
    pub motion: DVec3,
    pub yaw: f32,
    pub pitch: f32,
    pub prev_yaw: f32,
    pub prev_pitch: f32,
    /// `EntityArrow.inGround`
    pub in_ground: bool,
}

/// `MathHelper.atan2(x, z)` in degrees and `atan2(y, horizontal)`: the angles a motion points at.
fn angles_of(m: DVec3) -> (f32, f32) {
    let f = (m.x * m.x + m.z * m.z).sqrt();
    ((m.x.atan2(m.z) * 57.295_776_367_187_5) as f32, (m.y.atan2(f) * 57.295_776_367_187_5) as f32)
}

impl Arrow {
    /// `handleSpawnObject` for an arrow: the packet's position and angles, then `setVelocity`.
    pub fn spawn(m: &SpawnObject) -> Arrow {
        let mut a = Arrow {
            pos: DVec3::new(m.pos[0], m.pos[1], m.pos[2]),
            prev_pos: DVec3::new(m.pos[0], m.pos[1], m.pos[2]),
            motion: DVec3::ZERO,
            yaw: m.yaw,
            pitch: m.pitch,
            prev_yaw: 0.0,
            prev_pitch: 0.0,
            in_ground: false,
        };
        a.set_velocity(DVec3::new(m.motion[0] as f64, m.motion[1] as f64, m.motion[2] as f64));
        a
    }

    /// `EntityArrow.setVelocity`: the motion, and the angles from it while the previous ones are zero.
    pub fn set_velocity(&mut self, m: DVec3) {
        self.motion = m;
        if self.prev_pitch == 0.0 && self.prev_yaw == 0.0 {
            (self.yaw, self.pitch) = angles_of(m);
            self.prev_yaw = self.yaw;
            self.prev_pitch = self.pitch;
        }
    }

    /// `handleEntityTeleport`: position, angles (`Entity.setRotation`), `onGround`.
    pub fn moved(&mut self, m: &EntityMove) {
        self.pos = DVec3::new(m.pos[0], m.pos[1], m.pos[2]);
        self.yaw = m.yaw;
        self.pitch = m.pitch;
        self.in_ground = m.on_ground;
    }

    /// The interpolated feet.
    pub fn feet(&self, partial: f32) -> DVec3 {
        self.prev_pos + (self.pos - self.prev_pos) * partial as f64
    }

    /// `EntityArrow.onUpdate` (client): `Entity.onEntityUpdate`'s previous values, the angles from
    /// the motion when both are zero, then the flight unless `inGround`.
    pub fn tick(&mut self, w: &impl BlockAccess) {
        self.prev_pos = self.pos;
        self.prev_pitch = self.pitch;
        self.prev_yaw = self.yaw;
        if self.prev_pitch == 0.0 && self.prev_yaw == 0.0 {
            (self.yaw, self.pitch) = angles_of(self.motion);
            self.prev_yaw = self.yaw;
            self.prev_pitch = self.pitch;
        }
        if self.in_ground {
            return;
        }
        // Entity.handleWaterMovement's box: 0.5 wide, 0.5 tall, grown by (0, -0.4, 0) and shrunk.
        let bb = Aabb::new(self.pos.x - 0.25, self.pos.y, self.pos.z - 0.25, self.pos.x + 0.25, self.pos.y + 0.5, self.pos.z + 0.25).grow(0.0, -0.400_000_005_960_464_5, 0.0).shrink(0.001);
        let in_water = handle_material_acceleration(w, &bb, Mat::Water);
        self.pos += self.motion;
        let f4 = (self.motion.x * self.motion.x + self.motion.z * self.motion.z).sqrt();
        self.yaw = (self.motion.x.atan2(self.motion.z) * 57.295_776_367_187_5) as f32;
        self.pitch = (self.motion.y.atan2(f4) * 57.295_776_367_187_5) as f32;
        while self.pitch - self.prev_pitch < -180.0 {
            self.prev_pitch -= 360.0;
        }
        while self.pitch - self.prev_pitch >= 180.0 {
            self.prev_pitch += 360.0;
        }
        while self.yaw - self.prev_yaw < -180.0 {
            self.prev_yaw -= 360.0;
        }
        while self.yaw - self.prev_yaw >= 180.0 {
            self.prev_yaw += 360.0;
        }
        self.pitch = self.prev_pitch + (self.pitch - self.prev_pitch) * 0.2;
        self.yaw = self.prev_yaw + (self.yaw - self.prev_yaw) * 0.2;
        let f1 = if in_water { 0.6f32 } else { 0.99f32 } as f64;
        self.motion *= f1;
        self.motion.y -= 0.050_000_000_745_058_06;
        // DROPPED: the ray trace; entering a block stops the arrow there.
        let p = self.pos;
        let probe = Aabb::new(p.x - 1.0e-3, p.y - 1.0e-3, p.z - 1.0e-3, p.x + 1.0e-3, p.y + 1.0e-3, p.z + 1.0e-3);
        if get_collision_boxes(w, &probe).iter().any(|b| b.min_x <= p.x && p.x <= b.max_x && b.min_y <= p.y && p.y <= b.max_y && b.min_z <= p.z && p.z <= b.max_z) {
            self.in_ground = true;
            self.motion = DVec3::ZERO;
        }
    }
}

/// The arrows of the session.
#[derive(Resource, Default)]
pub struct Arrows {
    pub map: HashMap<u32, Arrow>,
    /// Entity per arrow id, for the draw.
    drawn: HashMap<u32, (Entity, Handle<StandardMaterial>)>,
}

impl Arrows {
    pub fn spawn(&mut self, m: &SpawnObject) {
        if matches!(m.data, ObjectData::Arrow { .. }) {
            self.map.insert(m.id, Arrow::spawn(m));
        }
    }

    pub fn moves(&mut self, moves: &[EntityMove]) {
        for m in moves {
            if let Some(a) = self.map.get_mut(&m.id) {
                a.moved(m);
            }
        }
    }

    pub fn velocity(&mut self, v: &Velocity) {
        if let Some(a) = self.map.get_mut(&v.id) {
            a.set_velocity(DVec3::new(v.motion[0] as f64, v.motion[1] as f64, v.motion[2] as f64));
        }
    }

    pub fn destroy(&mut self, ids: &[u32]) {
        for id in ids {
            self.map.remove(id);
        }
    }

    /// One client tick for every arrow in a loaded chunk.
    pub fn tick(&mut self, w: &impl BlockAccess) {
        for a in self.map.values_mut() {
            if !w.loaded(floor(a.pos.x), floor(a.pos.z)) {
                continue;
            }
            a.tick(w);
        }
    }
}

/// `RenderArrow.doRender`'s matrix up to the vertices: translate, `rotate(yaw - 90, y)`,
/// `rotate(pitch, z)`, `rotate(45, x)`, scale 0.05625, `translate(-4, 0, 0)`.
pub fn arrow_matrix(at: Vec3, yaw: f32, pitch: f32) -> Mat4 {
    Mat4::from_translation(at)
        * Mat4::from_rotation_y((yaw - 90.0).to_radians())
        * Mat4::from_rotation_z(pitch.to_radians())
        * Mat4::from_rotation_x(45.0f32.to_radians())
        * Mat4::from_scale(Vec3::splat(0.056_25))
        * Mat4::from_translation(Vec3::new(-4.0, 0.0, 0.0))
}

/// The six quads of `doRender`: the back cross (two faces at x = -7) and the four long fins, each
/// rotated a further 90 degrees about x. Corners and uvs in model units.
pub fn arrow_quads() -> Vec<([Vec3; 4], [[f32; 2]; 4])> {
    let v = |x: f32, y: f32, z: f32| Vec3::new(x, y, z);
    let mut out = vec![
        ([v(-7.0, -2.0, -2.0), v(-7.0, -2.0, 2.0), v(-7.0, 2.0, 2.0), v(-7.0, 2.0, -2.0)], [[0.0, 0.15625], [0.15625, 0.15625], [0.15625, 0.3125], [0.0, 0.3125]]),
        ([v(-7.0, 2.0, -2.0), v(-7.0, 2.0, 2.0), v(-7.0, -2.0, 2.0), v(-7.0, -2.0, -2.0)], [[0.0, 0.15625], [0.15625, 0.15625], [0.15625, 0.3125], [0.0, 0.3125]]),
    ];
    for j in 0..4 {
        let r = Mat4::from_rotation_x((90.0 * (j + 1) as f32).to_radians());
        let c = [v(-8.0, -2.0, 0.0), v(8.0, -2.0, 0.0), v(8.0, 2.0, 0.0), v(-8.0, 2.0, 0.0)].map(|p| r.transform_point3(p));
        out.push((c, [[0.0, 0.0], [0.5, 0.0], [0.5, 0.15625], [0.0, 0.15625]]));
    }
    out
}

fn arrow_mesh() -> Mesh {
    let (mut pos, mut uv, mut idx) = (Vec::new(), Vec::new(), Vec::new());
    for (corners, uvs) in arrow_quads() {
        let base = pos.len() as u32;
        pos.extend(corners.map(|c| c.to_array()));
        uv.extend(uvs);
        idx.extend([base, base + 1, base + 2, base, base + 2, base + 3]);
    }
    let n = pos.len();
    Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, pos)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.0, 1.0, 0.0]; n])
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uv)
        .with_inserted_indices(Indices::U32(idx))
}

fn read_frames(
    mut arrows: ResMut<Arrows>,
    mut spawns: MessageReader<SpawnMsg>,
    mut moves: MessageReader<EntityMoves>,
    mut velocity: MessageReader<VelocityMsg>,
    mut destroy: MessageReader<Destroy>,
) {
    for SpawnMsg(m) in spawns.read() {
        arrows.spawn(m);
    }
    for EntityMoves(m) in moves.read() {
        arrows.moves(m);
    }
    for VelocityMsg(v) in velocity.read() {
        arrows.velocity(v);
    }
    for Destroy(ids) in destroy.read() {
        arrows.destroy(ids);
    }
}

fn tick(clock: Res<Clock>, streamer: Res<Streamer>, mut arrows: ResMut<Arrows>) {
    for _ in 0..clock.ticks_this_frame {
        arrows.tick(&*streamer);
    }
}

#[allow(clippy::too_many_arguments)]
fn draw(
    mut commands: Commands,
    clock: Res<Clock>,
    view: Res<View>,
    env: Res<Env>,
    streamer: Res<Streamer>,
    image: Option<Res<ArrowImage>>,
    mut arrows: ResMut<Arrows>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut shared: Local<Option<Handle<Mesh>>>,
    mut shown: Query<&mut Transform>,
) {
    let Some(image) = image else { return };
    let mesh = shared.get_or_insert_with(|| meshes.add(arrow_mesh())).clone();
    let partial = clock.partial;
    let eye = view.eye.as_dvec3();
    let arrows = &mut *arrows;
    let gone: Vec<u32> = arrows.drawn.keys().filter(|id| !arrows.map.contains_key(id)).copied().collect();
    for id in gone {
        if let Some((e, m)) = arrows.drawn.remove(&id) {
            commands.entity(e).despawn();
            materials.remove(&m);
        }
    }
    for (id, a) in &arrows.map {
        // Entity.isInRangeToRenderDist: 64 * the box's average edge (0.5), scaled.
        if a.feet(partial).distance_squared(eye) > 64.0 * 64.0 {
            continue;
        }
        let yaw = a.prev_yaw + (a.yaw - a.prev_yaw) * partial;
        let pitch = a.prev_pitch + (a.pitch - a.prev_pitch) * partial;
        let m = arrow_matrix(a.feet(partial).as_vec3(), yaw, pitch);
        // getBrightnessForRender: the lightmap at the arrow (its eye height is 0).
        let l = entity_light(&env, &streamer, a.pos);
        let tint = Color::srgb(l[0], l[1], l[2]);
        match arrows.drawn.get(id) {
            Some((e, mat)) => {
                if let Ok(mut t) = shown.get_mut(*e) {
                    *t = Transform::from_matrix(m);
                }
                if materials.get(mat).is_some_and(|x| x.base_color != tint) {
                    if let Some(mut x) = materials.get_mut(mat) {
                        x.base_color = tint;
                    }
                }
            }
            None => {
                let mat = materials.add(StandardMaterial {
                    base_color: tint,
                    base_color_texture: Some(image.0.clone()),
                    unlit: true,
                    double_sided: true,
                    cull_mode: None,
                    fog_enabled: false,
                    alpha_mode: AlphaMode::Mask(0.1),
                    ..default()
                });
                let e = commands
                    .spawn((Mesh3d(mesh.clone()), MeshMaterial3d(mat.clone()), Transform::from_matrix(m), Visibility::Inherited, NoFrustumCulling, NotShadowCaster))
                    .id();
                arrows.drawn.insert(*id, (e, mat));
            }
        }
    }
}

pub fn plugin(app: &mut App) {
    app.init_resource::<Arrows>().add_systems(
        Update,
        (
            read_frames,
            tick.after(crate::player::run_ticks).after(read_frames).run_if(resource_exists::<Streamer>).run_if(resource_exists::<Clock>),
            draw.after(crate::sky::advance_clock).after(read_frames).after(tick).run_if(resource_exists::<Streamer>).run_if(resource_exists::<Clock>).run_if(resource_exists::<View>).run_if(resource_exists::<Env>),
        ),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::collision::tests::Blocks;

    fn shot(motion: [f32; 3]) -> SpawnObject {
        SpawnObject { id: 3, pos: [0.5, 70.0, 0.5], motion, data: ObjectData::Arrow { shooter: 1 }, ..Default::default() }
    }

    #[test]
    fn an_arrow_falls_by_gravity_and_drag() {
        let mut a = Arrow::spawn(&shot([1.0, 0.0, 0.0]));
        let w = Blocks::default();
        a.tick(&w);
        // Moved by the old motion, then drag 0.99 and gravity 0.05.
        assert!((a.pos.x - 1.5).abs() < 1.0e-9 && (a.pos.y - 70.0).abs() < 1.0e-9);
        assert!((a.motion.x - 0.99f32 as f64).abs() < 1.0e-9);
        assert!((a.motion.y - -0.050_000_000_745_058_06).abs() < 1.0e-12);
        a.tick(&w);
        assert!((a.pos.y - (70.0 - 0.050_000_000_745_058_06)).abs() < 1.0e-9);
        assert!((a.motion.y - (-0.050_000_000_745_058_06 * 0.99f32 as f64 - 0.050_000_000_745_058_06)).abs() < 1.0e-9);
    }

    #[test]
    fn an_arrow_in_water_drags_by_0_6() {
        let mut a = Arrow::spawn(&shot([1.0, 0.0, 0.0]));
        let mut w = Blocks::default();
        for x in -2..4 {
            for y in 68..72 {
                for z in -2..4 {
                    w.set(x, y, z, worldgen::blocks::WATER);
                }
            }
        }
        a.tick(&w);
        assert!((a.motion.x - 0.6f32 as f64).abs() < 1.0e-9, "{}", a.motion.x);
    }

    #[test]
    fn a_stuck_arrow_does_not_move_and_a_move_frame_sticks_it() {
        let mut a = Arrow::spawn(&shot([1.0, 0.5, 0.0]));
        let w = Blocks::default();
        a.moved(&EntityMove { id: 3, pos: [4.0, 71.0, 4.0], yaw: 90.0, pitch: 10.0, on_ground: true, ..Default::default() });
        assert!(a.in_ground);
        let before = (a.pos, a.motion);
        for _ in 0..5 {
            a.tick(&w);
        }
        assert_eq!((a.pos, a.motion), before);
        assert_eq!(a.pos, DVec3::new(4.0, 71.0, 4.0));
        // A flying arrow that the server frame leaves in the air keeps going.
        let mut b = Arrow::spawn(&shot([0.0, 0.0, 1.0]));
        b.moved(&EntityMove { id: 3, pos: [0.0, 70.0, 0.0], yaw: 0.0, pitch: 0.0, on_ground: false, ..Default::default() });
        b.tick(&w);
        assert!(b.pos.z > 0.9);
    }

    #[test]
    fn the_angles_come_from_the_motion_and_an_arrow_in_a_block_stops() {
        let a = Arrow::spawn(&shot([0.0, 0.0, 1.0]));
        assert_eq!((a.yaw, a.pitch), (0.0, 0.0));
        let a = Arrow::spawn(&shot([1.0, 0.0, 0.0]));
        assert!((a.yaw - 90.0).abs() < 1.0e-4);
        let mut b = Arrow::spawn(&shot([0.0, -1.0, 0.0]));
        let w = Blocks::floor_at(68, 4);
        b.pos = DVec3::new(0.5, 69.5, 0.5);
        b.tick(&w);
        assert!(b.in_ground && b.motion == DVec3::ZERO);
    }

    #[test]
    fn the_arrow_has_six_quads_and_faces_along_its_motion() {
        assert_eq!(arrow_quads().len(), 6);
        // The tip (x = 8 in model units, after translate -4) lies 0.05625 * 4 ahead along +x
        // when yaw is 90 (rotate yaw - 90 = 0) and pitch 0.
        let m = arrow_matrix(Vec3::ZERO, 90.0, 0.0);
        let tip = m.transform_point3(Vec3::new(8.0, 0.0, 0.0));
        assert!((tip.x - 0.056_25 * 4.0).abs() < 1.0e-5 && tip.y.abs() < 1.0e-5 && tip.z.abs() < 1.0e-5, "{tip}");
        // Pitch 90 lifts the tip straight up.
        let up = arrow_matrix(Vec3::ZERO, 90.0, 90.0).transform_point3(Vec3::new(8.0, 0.0, 0.0));
        assert!((up.y - 0.225).abs() < 1.0e-5 && up.x.abs() < 1.0e-5, "{up}");
    }
}
