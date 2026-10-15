//! The crit particles of a hit: `S_ANIMATION` `ANIM_CRIT` and `ANIM_MAGIC_CRIT` are
//! `NetHandlerPlayClient.handleAnimation` types 4 and 5, `ParticleManager.emitParticleAtEntity`.
//!
//! An emitter ([`Emitter`], `ParticleEmitter`) lives 3 ticks, and each tick, constructor
//! included, tries 16 random points in the unit sphere and spawns a `ParticleCrit` at the
//! entity's centre plus that point scaled by a quarter of its size, moving at `(d0, d1 + 0.2, d2)`.
//! [`Crit`] is `ParticleCrit` itself (`Particle.onUpdate` and `move`, texture 65 of particles.png,
//! 66 for the magic variant, shrinking in over its first 1/32 of life), drawn as camera facing
//! quads in one mesh rebuilt each frame.
//!
//! `S_ENTITY_EVENT` [`proto::EV_LOVE`] is `EntityAnimal.handleStatusUpdate(18)`: seven `ParticleHeart`s
//! (texture 80, [`HEART_INDEX`]) around the animal ([`Crit::hearts`]), kept in the same list and
//! drawn the same way; [`step`] runs `ParticleHeart.onUpdate` for them.
//!
//! Each quad's color is multiplied by the lightmap at the particle's block
//! (`Particle.getBrightnessForRender`, 0 where the chunk is not loaded).
//!
//! DROPPED: the fog on the quads, the 32 block spawn distance check of
//! `spawnParticle`, and `ParticleManager`'s per-layer cap of 16384.

use bevy::asset::RenderAssetUsages;
use bevy::camera::visibility::NoFrustumCulling;
use bevy::light::NotShadowCaster;
use bevy::math::DVec3;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use worldgen::JavaRandom;

use super::{Animation, EntityEvent};
use crate::collision::{get_collision_boxes, Aabb, BlockAccess};
use crate::env::Env;
use crate::interact::Controller;
use crate::sky::{Clock, View};
use crate::stream::Streamer;

/// `ParticleEmitter`'s `maxAge` for a crit.
const EMITTER_TICKS: u32 = 3;
const MAX: usize = 4000;
/// `ParticleCrit`: `setParticleTextureIndex(65)`, and 66 after `nextTextureIndexX`.
pub const CRIT_INDEX: u32 = 65;
/// `ParticleHeart`: `setParticleTextureIndex(80)`.
pub const HEART_INDEX: u32 = 80;
/// `ParticleHeart`'s `particleMaxAge`.
const HEART_AGE: i32 = 16;

/// `particles.png`, loaded at startup (main.rs).
#[derive(Resource)]
pub struct CritImage(pub Handle<Image>);

#[derive(Clone, Debug)]
pub struct Particle {
    prev: DVec3,
    pos: DVec3,
    motion: DVec3,
    bb: Aabb,
    on_ground: bool,
    age: i32,
    max_age: i32,
    o_size: f32,
    rgb: [f32; 3],
    index: u32,
}

#[derive(Clone, Copy, Debug)]
pub struct Emitter {
    id: u32,
    age: u32,
    magic: bool,
}

#[derive(Resource)]
pub struct Crit {
    list: Vec<Particle>,
    emitters: Vec<Emitter>,
    rand: JavaRandom,
    mesh: Handle<Mesh>,
}

/// `Particle.setPosition` for the 0.2 box: centred on x and z, standing on y.
fn box_at(p: DVec3) -> Aabb {
    Aabb::new(p.x - 0.1, p.y, p.z - 0.1, p.x + 0.1, p.y + 0.2, p.z + 0.1)
}

impl Crit {
    fn new(mesh: Handle<Mesh>) -> Crit {
        Crit { list: Vec::new(), emitters: Vec::new(), rand: JavaRandom::new(0x1F3A5C7), mesh }
    }

    /// `new ParticleCrit(world, x, y, z, xSpeed, ySpeed, zSpeed)`, then `onUpdate()` once as the
    /// constructor of `ParticleEmitter` does through `spawnParticle` (which only adds it).
    fn spawn(&mut self, at: DVec3, speed: DVec3, magic: bool) {
        if self.list.len() >= MAX {
            self.list.remove(0);
        }
        let r = &mut self.rand;
        // Particle(World, x, y, z): size 0.2, jitter, scale and life.
        let _jitter = (r.next_float() * 3.0, r.next_float() * 3.0);
        let scale = (r.next_float() * 0.5 + 0.5) * 2.0;
        let _life = (4.0 / (r.next_float() * 0.9 + 0.1)) as i32;
        // Particle(World, x, y, z, xSpeed, ySpeed, zSpeed) with zero speeds
        let mut m = DVec3::new(
            (r.next_double() * 2.0 - 1.0) * 0.4000000059604645,
            (r.next_double() * 2.0 - 1.0) * 0.4000000059604645,
            (r.next_double() * 2.0 - 1.0) * 0.4000000059604645,
        );
        let f = ((r.next_double() + r.next_double() + 1.0) as f32 * 0.15) as f64;
        let f1 = (m.length() as f32) as f64;
        m = m / f1 * f * 0.4000000059604645;
        m.y += 0.10000000149011612;
        // ParticleCrit
        m = m * 0.10000000149011612 + speed * 0.4000000059604645;
        let g = r.next_float() * 0.3 + 0.6;
        let o_size = scale * 0.75;
        let mut rgb = [g; 3];
        if magic {
            rgb = [g * 0.3, g * 0.8, g];
        }
        let max_age = (6.0 / (r.next_float() * 0.8 + 0.6)) as i32;
        let mut p = Particle { prev: at, pos: at, motion: m, bb: box_at(at), on_ground: false, age: 0, max_age, o_size, rgb, index: CRIT_INDEX + magic as u32 };
        step(&mut p, None::<&Streamer>);
        self.list.push(p);
    }

    /// `new ParticleHeart(world, x, y, z, ..)` through `ParticleManager.spawnEffectParticle`: the
    /// `Particle` constructor's draws, motion times 0.01 plus 0.1 up, scale `* 0.75 * 2`, age 16.
    /// The speeds the factory is handed are not used by the constructor. No update runs before it
    /// is added.
    fn spawn_heart(&mut self, at: DVec3) {
        if self.list.len() >= MAX {
            self.list.remove(0);
        }
        let r = &mut self.rand;
        let _jitter = (r.next_float() * 3.0, r.next_float() * 3.0);
        let scale = (r.next_float() * 0.5 + 0.5) * 2.0;
        let _life = (4.0 / (r.next_float() * 0.9 + 0.1)) as i32;
        let mut m = DVec3::new(
            (r.next_double() * 2.0 - 1.0) * 0.4000000059604645,
            (r.next_double() * 2.0 - 1.0) * 0.4000000059604645,
            (r.next_double() * 2.0 - 1.0) * 0.4000000059604645,
        );
        let f = ((r.next_double() + r.next_double() + 1.0) as f32 * 0.15) as f64;
        let f1 = (m.length() as f32) as f64;
        m = m / f1 * f * 0.4000000059604645;
        m.y += 0.10000000149011612;
        m *= 0.009999999776482582;
        m.y += 0.1;
        let o_size = scale * 0.75 * 2.0;
        self.list.push(Particle { prev: at, pos: at, motion: m, bb: box_at(at), on_ground: false, age: 0, max_age: HEART_AGE, o_size, rgb: [1.0; 3], index: HEART_INDEX });
    }

    /// `EntityAnimal.handleStatusUpdate(18)`: seven hearts at `posX + nextFloat * width * 2 - width`,
    /// `posY + 0.5 + nextFloat * height`, likewise z. `bb` is the animal's box (feet at `min_y`).
    /// DROPPED: the three `nextGaussian` speeds, which `ParticleHeart` ignores.
    pub fn hearts(&mut self, bb: &Aabb) {
        let (width, height) = ((bb.max_x - bb.min_x) as f32, (bb.max_y - bb.min_y) as f32);
        let c = bb.center();
        for _ in 0..7 {
            let x = c.x + (self.rand.next_float() * width * 2.0) as f64 - width as f64;
            let y = bb.min_y + 0.5 + (self.rand.next_float() * height) as f64;
            let z = c.z + (self.rand.next_float() * width * 2.0) as f64 - width as f64;
            self.spawn_heart(DVec3::new(x, y, z));
        }
    }

    /// `ParticleEmitter.onUpdate`: sixteen tries at a point of the unit sphere around the entity.
    fn emit(&mut self, bb: &Aabb, magic: bool) {
        let (width, height) = (bb.max_x - bb.min_x, bb.max_y - bb.min_y);
        let c = bb.center();
        for _ in 0..16 {
            let d0 = self.rand.next_double() * 2.0 - 1.0;
            let d1 = self.rand.next_double() * 2.0 - 1.0;
            let d2 = self.rand.next_double() * 2.0 - 1.0;
            if d0 * d0 + d1 * d1 + d2 * d2 > 1.0 {
                continue;
            }
            let at = DVec3::new(c.x + d0 * width / 4.0, bb.min_y + height / 2.0 + d1 * height / 4.0, c.z + d2 * width / 4.0);
            self.spawn(at, DVec3::new(d0, d1 + 0.2, d2), magic);
        }
    }

    /// `emitParticleAtEntity`: the emitter's constructor runs one update at once.
    pub fn emit_at(&mut self, id: u32, bb: &Aabb, magic: bool) {
        self.emit(bb, magic);
        self.emitters.push(Emitter { id, age: 1, magic });
    }

    /// One client tick: the emitters, then every particle.
    fn tick(&mut self, w: &impl BlockAccess, boxes: &[(u32, Aabb)]) {
        let mut emitters = std::mem::take(&mut self.emitters);
        emitters.retain_mut(|e| {
            if let Some((_, bb)) = boxes.iter().find(|(id, _)| *id == e.id) {
                self.emit(bb, e.magic);
            }
            e.age += 1;
            e.age < EMITTER_TICKS
        });
        self.emitters = emitters;
        self.list.retain_mut(|p| step(p, Some(w)));
    }
}

/// `ParticleCrit.onUpdate`; false once expired. With no world it only moves, as the constructor's
/// first update has nothing to collide with before the particle is added.
fn step<W: BlockAccess>(p: &mut Particle, w: Option<&W>) -> bool {
    p.prev = p.pos;
    let alive = p.age < p.max_age;
    p.age += 1;
    if !alive {
        return false;
    }
    let (mut x, mut y, mut z) = (p.motion.x, p.motion.y, p.motion.z);
    let y0 = y;
    let heart = p.index == HEART_INDEX;
    let boxes = w.map(|w| get_collision_boxes(w, &p.bb.expand(x, y, z))).unwrap_or_default();
    for b in &boxes {
        y = b.calculate_y_offset(&p.bb, y);
    }
    p.bb = p.bb.offset(0.0, y, 0.0);
    for b in &boxes {
        x = b.calculate_x_offset(&p.bb, x);
    }
    p.bb = p.bb.offset(x, 0.0, 0.0);
    for b in &boxes {
        z = b.calculate_z_offset(&p.bb, z);
    }
    p.bb = p.bb.offset(0.0, 0.0, z);
    p.pos = DVec3::new((p.bb.min_x + p.bb.max_x) / 2.0, p.bb.min_y, (p.bb.min_z + p.bb.max_z) / 2.0);
    p.on_ground = y0 != y && y0 < 0.0;
    if x != p.motion.x {
        p.motion.x = 0.0;
    }
    if z != p.motion.z {
        p.motion.z = 0.0;
    }
    if heart {
        // ParticleHeart.onUpdate: the same move, then 1.1 on the ground plane when y did not change.
        if p.pos.y == p.prev.y {
            p.motion.x *= 1.1;
            p.motion.z *= 1.1;
        }
        p.motion *= 0.8600000143051147;
        if p.on_ground {
            p.motion.x *= 0.699999988079071;
            p.motion.z *= 0.699999988079071;
        }
        return true;
    }
    p.rgb[1] *= 0.96;
    p.rgb[2] *= 0.9;
    p.motion *= 0.699999988079071;
    p.motion.y -= 0.019999999552965164;
    if p.on_ground {
        p.motion.x *= 0.699999988079071;
        p.motion.z *= 0.699999988079071;
    }
    true
}

/// `ParticleCrit.renderParticle`'s size: `oSize * clamp((age + partial) / maxAge * 32, 0, 1)`.
pub fn render_scale(o_size: f32, age: i32, max_age: i32, partial: f32) -> f32 {
    o_size * ((age as f32 + partial) / max_age as f32 * 32.0).clamp(0.0, 1.0)
}

/// The texture cell of a particle index: `u0, v0` and the `0.0624375` span of `renderParticle`.
pub fn uv_of(index: u32) -> (f32, f32, f32, f32) {
    let (u0, v0) = ((index % 16) as f32 / 16.0, (index / 16) as f32 / 16.0);
    (u0, v0, u0 + 0.0624375, v0 + 0.0624375)
}

pub fn spawn_mesh(mut commands: Commands, mut meshes: ResMut<Assets<Mesh>>, mut materials: ResMut<Assets<StandardMaterial>>, image: Res<CritImage>) {
    let mesh = meshes.add(build(&[], Vec3::ZERO, Vec3::X, Vec3::Y, 0.0, DVec3::ZERO, &|_| [1.0; 3]));
    commands.insert_resource(Crit::new(mesh.clone()));
    commands.spawn((
        CritMesh,
        Mesh3d(mesh),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color_texture: Some(image.0.clone()),
            unlit: true,
            double_sided: true,
            cull_mode: None,
            fog_enabled: false,
            alpha_mode: AlphaMode::Mask(0.003921569),
            ..default()
        })),
        Transform::default(),
        Visibility::Hidden,
        NoFrustumCulling,
        NotShadowCaster,
    ));
}

#[derive(Component)]
pub struct CritMesh;

/// The quads of every particle, in blocks from `origin`; an empty list gives one degenerate quad,
/// as the mesh allocator refuses an empty mesh.
fn build(list: &[Particle], normal: Vec3, right: Vec3, up: Vec3, pt: f32, origin: DVec3, light: &dyn Fn(DVec3) -> [f32; 3]) -> Mesh {
    let mut pos: Vec<[f32; 3]> = Vec::with_capacity(list.len() * 4 + 4);
    let mut uv: Vec<[f32; 2]> = Vec::new();
    let mut col: Vec<[f32; 4]> = Vec::new();
    let mut idx: Vec<u32> = Vec::new();
    let mut quad = |corners: [Vec3; 4], uvs: [[f32; 2]; 4], c: [f32; 4]| {
        let base = pos.len() as u32;
        pos.extend(corners.map(|v| v.to_array()));
        uv.extend(uvs);
        col.extend([c; 4]);
        idx.extend([base, base + 1, base + 2, base, base + 2, base + 3]);
    };
    for p in list {
        let at = (p.prev + (p.pos - p.prev) * pt as f64 - origin).as_vec3();
        let s = 0.1 * render_scale(p.o_size, p.age, p.max_age, pt);
        let (r, u) = (right * s, up * s);
        let (u0, v0, u1, v1) = uv_of(p.index);
        let l = light(p.prev + (p.pos - p.prev) * pt as f64);
        let c = Color::srgb(p.rgb[0] * l[0], p.rgb[1] * l[1], p.rgb[2] * l[2]).to_linear();
        quad([at - r - u, at - r + u, at + r + u, at + r - u], [[u0, v1], [u0, v0], [u1, v0], [u1, v1]], [c.red, c.green, c.blue, 1.0]);
    }
    if list.is_empty() {
        quad([Vec3::ZERO; 4], [[0.0; 2]; 4], [1.0; 4]);
    }
    let n = pos.len();
    Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, pos)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, vec![normal.to_array(); n])
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uv)
        .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, col)
        .with_inserted_indices(Indices::U32(idx))
}

/// `Particle.getBrightnessForRender`: the combined light of the block at `at`, the lightmap's RGB.
pub fn particle_light(env: &Env, s: &Streamer, at: DVec3) -> [f32; 3] {
    match s.light_at(at.x.floor() as i32, (at.y.floor() as i32).clamp(0, 255), at.z.floor() as i32) {
        Some((sky, block)) => env.light_rgb(block as f32 * 16.0, sky as f32 * 16.0),
        None => env.light_rgb(0.0, 0.0),
    }
}

/// `handleAnimation` 4 and 5.
fn animations(mut msgs: MessageReader<Animation>, ctl: Res<Controller>, mut crit: ResMut<Crit>) {
    for Animation(a) in msgs.read() {
        let magic = match a.animation {
            proto::ANIM_CRIT => false,
            proto::ANIM_MAGIC_CRIT => true,
            _ => continue,
        };
        if let Some((_, bb)) = ctl.entities.iter().find(|(id, _)| *id == a.id) {
            crit.emit_at(a.id, bb, magic);
        }
    }
}

/// `handleStatusUpdate` 18 on an animal.
fn love(mut msgs: MessageReader<EntityEvent>, ctl: Res<Controller>, mut crit: ResMut<Crit>) {
    for EntityEvent(e) in msgs.read() {
        if e.status != proto::EV_LOVE {
            continue;
        }
        if let Some((_, bb)) = ctl.entities.iter().find(|(id, _)| *id == e.id) {
            crit.hearts(bb);
        }
    }
}

fn tick(clock: Res<Clock>, streamer: Res<Streamer>, ctl: Res<Controller>, mut crit: ResMut<Crit>) {
    for _ in 0..clock.ticks_this_frame {
        crit.tick(&*streamer, &ctl.entities);
    }
}

fn draw(clock: Res<Clock>, view: Res<View>, env: Res<Env>, streamer: Res<Streamer>, crit: Res<Crit>, mut meshes: ResMut<Assets<Mesh>>, entity: Single<(&mut Transform, &mut Visibility), With<CritMesh>>) {
    let (mut transform, mut visibility) = entity.into_inner();
    if crit.list.is_empty() {
        *visibility = Visibility::Hidden;
        return;
    }
    // ActiveRenderInfo's billboard axes.
    let (yaw, pitch) = (view.yaw.to_radians(), view.pitch.to_radians());
    let right = Vec3::new(yaw.cos(), 0.0, yaw.sin());
    let up = Vec3::new(-yaw.sin() * pitch.sin(), pitch.cos(), yaw.cos() * pitch.sin());
    let origin = DVec3::new((view.eye.x.floor() as i32 & !15) as f64, 0.0, (view.eye.z.floor() as i32 & !15) as f64);
    let _ = meshes.insert(&crit.mesh, build(&crit.list, -view.look, right, up, clock.partial, origin, &|at| particle_light(&env, &streamer, at)));
    *transform = Transform::from_xyz(origin.x as f32, 0.0, origin.z as f32);
    *visibility = Visibility::Inherited;
}

pub fn plugin(app: &mut App) {
    app.add_systems(
        Update,
        (
            animations.after(crate::players::apply_events).before(crate::player::run_ticks),
            love.after(crate::players::apply_events).before(crate::player::run_ticks),
            tick.after(crate::player::run_ticks),
            draw.after(crate::sky::update_view).after(tick),
        )
            .run_if(resource_exists::<Crit>)
            .run_if(resource_exists::<Controller>)
            .run_if(resource_exists::<Env>)
            .run_if(resource_exists::<Streamer>),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::collision::tests::Blocks;

    fn cow() -> Aabb {
        Aabb::new(-0.45, 64.0, -0.45, 0.45, 65.4, 0.45)
    }

    fn crit() -> Crit {
        Crit::new(Handle::default())
    }

    #[test]
    fn an_emitter_runs_three_batches_and_then_expires() {
        let mut c = crit();
        c.emit_at(5, &cow(), false);
        let first = c.list.len();
        assert!(first > 0 && first <= 16, "one batch from the constructor: {first}");
        assert_eq!(c.emitters.len(), 1);
        let boxes = [(5, cow())];
        c.tick(&Blocks::floor_at(63, 4), &boxes);
        assert_eq!(c.emitters.len(), 1, "two updates so far");
        c.tick(&Blocks::floor_at(63, 4), &boxes);
        assert!(c.emitters.is_empty(), "the third update expires it");
    }

    #[test]
    fn crits_spawn_around_the_middle_of_the_entity() {
        let mut c = crit();
        c.emit(&cow(), false);
        for p in &c.list {
            // Spawned inside the quarter-size sphere, then moved once by `onUpdate`.
            assert!(p.prev.x.abs() <= 0.225 + 1e-9 && p.prev.z.abs() <= 0.225 + 1e-9, "{:?}", p.prev);
            assert!((p.prev.y - 64.7).abs() <= 0.35 + 1e-9, "{:?}", p.prev);
            assert_eq!(p.index, 65);
        }
        let mut m = crit();
        m.emit(&cow(), true);
        assert!(m.list.iter().all(|p| p.index == 66 && p.rgb[0] < p.rgb[2]), "the magic variant is blue and uses the next cell");
    }

    #[test]
    fn a_crit_fades_in_over_a_thirty_second_of_its_life_and_dies_at_its_age() {
        assert_eq!(render_scale(1.5, 0, 8, 0.0), 0.0);
        assert!((render_scale(1.5, 0, 32, 0.5) - 0.75).abs() < 1e-6, "(age + partial) / maxAge * 32 = 0.5");
        assert_eq!(render_scale(1.5, 4, 8, 0.0), 1.5, "full size past 1/32 of the life");
        let mut p = Particle { prev: DVec3::ZERO, pos: DVec3::ZERO, motion: DVec3::ZERO, bb: box_at(DVec3::ZERO), on_ground: false, age: 0, max_age: 3, o_size: 1.0, rgb: [1.0; 3], index: 65 };
        let w = Blocks::default();
        assert!(step(&mut p, Some(&w)) && step(&mut p, Some(&w)) && step(&mut p, Some(&w)));
        assert!(!step(&mut p, Some(&w)), "gone once age reaches maxAge");
    }

    #[test]
    fn the_cell_of_index_65_is_one_across_and_four_down() {
        let (u0, v0, u1, v1) = uv_of(CRIT_INDEX);
        assert_eq!((u0, v0), (1.0 / 16.0, 4.0 / 16.0));
        assert!((u1 - u0 - 0.0624375).abs() < 1e-7 && (v1 - v0 - 0.0624375).abs() < 1e-7);
        assert_eq!(uv_of(66).0, 2.0 / 16.0);
    }

    #[test]
    fn seven_hearts_spawn_around_the_animal_with_the_constructors_motion() {
        let mut c = crit();
        c.hearts(&cow());
        assert_eq!(c.list.len(), 7);
        for p in &c.list {
            // x within width of the centre, y 0.5 to 0.5 + height above the feet.
            assert!(p.prev.x.abs() <= 0.9 + 1e-6 && p.prev.z.abs() <= 0.9 + 1e-6, "{:?}", p.prev);
            assert!(p.prev.y >= 64.5 && p.prev.y <= 64.5 + 1.4 + 1e-6, "{:?}", p.prev);
            assert_eq!((p.index, p.max_age, p.age, p.rgb), (80, 16, 0, [1.0; 3]));
            // 0.01 of a length at most 0.4 * 0.45, plus 0.1 up.
            assert!(p.motion.length() - 0.1 < 0.0018 + 1e-9 && (p.motion.y - 0.1).abs() < 0.0018, "{:?}", p.motion);
            // scale (1 to 2) * 0.75 * 2
            assert!(p.o_size >= 1.5 && p.o_size <= 3.0, "{}", p.o_size);
        }
        assert_eq!(uv_of(HEART_INDEX).0, 0.0);
        assert_eq!(uv_of(HEART_INDEX).1, 5.0 / 16.0);
    }

    #[test]
    fn a_heart_rises_and_slows_by_0_86_and_lives_sixteen_ticks() {
        let mut p = Particle { prev: DVec3::new(0.0, 66.0, 0.0), pos: DVec3::new(0.0, 66.0, 0.0), motion: DVec3::new(0.0, 0.1, 0.0), bb: box_at(DVec3::new(0.0, 66.0, 0.0)), on_ground: false, age: 0, max_age: 16, o_size: 1.0, rgb: [1.0; 3], index: HEART_INDEX };
        let w = Blocks::floor_at(63, 4);
        assert!(step(&mut p, Some(&w)));
        assert!((p.pos.y - 66.1).abs() < 1e-9, "moved by the old motion");
        assert!((p.motion.y - 0.1 * 0.8600000143051147).abs() < 1e-12, "no gravity, just 0.86");
        assert_eq!(p.rgb, [1.0; 3], "heart keeps its colour");
        // On the floor with a sideways push: y does not change, so x grows by 1.1 first.
        let mut g = Particle { prev: DVec3::new(0.0, 64.0, 0.0), pos: DVec3::new(0.0, 64.0, 0.0), motion: DVec3::new(0.01, 0.0, 0.0), bb: box_at(DVec3::new(0.0, 64.0, 0.0)), ..p.clone() };
        step(&mut g, Some(&w));
        assert!((g.motion.x - 0.01 * 1.1 * 0.8600000143051147).abs() < 1e-12);
        let mut n = 1;
        while step(&mut p, Some(&w)) {
            n += 1;
        }
        assert_eq!(n, 16, "alive for sixteen updates");
    }

    #[test]
    fn a_crit_loses_green_and_blue_and_sinks() {
        let mut p = Particle { prev: DVec3::ZERO, pos: DVec3::ZERO, motion: DVec3::new(0.0, 0.1, 0.0), bb: box_at(DVec3::ZERO), on_ground: false, age: 0, max_age: 20, o_size: 1.0, rgb: [1.0; 3], index: 65 };
        step(&mut p, Some(&Blocks::default()));
        assert_eq!(p.rgb[0], 1.0);
        assert!((p.rgb[1] - 0.96).abs() < 1e-6 && (p.rgb[2] - 0.9).abs() < 1e-6);
        assert!((p.motion.y - (0.1 * 0.7 - 0.02)).abs() < 1e-9);
    }
}
