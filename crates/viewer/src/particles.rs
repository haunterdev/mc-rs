//! Particles: `ParticleManager` with `ParticleDigging`, the block debris from breaking a block
//! (`addBlockDestroyEffects`) and the dust under a sprinting player (`createRunningParticles`).
//!
//! They tick with the client ticks (`Particle.onUpdate`, `Particle.move` with block collision)
//! and draw as camera-facing quads in the packed chunk vertex (vertex.rs), so the cutout terrain
//! material lights them through the lightmap and fogs them like the blocks they came from. The
//! mesh is rebuilt each frame around the camera's chunk.

use bevy::camera::visibility::NoFrustumCulling;
use bevy::light::NotShadowCaster;
use bevy::math::DVec3;
use bevy::prelude::*;
use worldgen::blocks::id;
use worldgen::populate::props::{material, PARTICLE_GRAVITY};
use worldgen::populate::state::mat;
use worldgen::JavaRandom;

use crate::collision::{get_collision_boxes, Aabb, BlockAccess};
use crate::colors::{biome_of, water_color};
use crate::mesher::FLAG_NOMIP;
use crate::sky::{Clock, View};
use crate::stream::{ChunkMaterials, Streamer};
use crate::vertex::{self, LayerMesh};
use crate::Atlas;

/// `ParticleManager`'s per-layer cap is 16384; far fewer ever show here.
const MAX: usize = 4000;

#[derive(Clone, Debug)]
pub struct Particle {
    prev: DVec3,
    pos: DVec3,
    motion: DVec3,
    bb: Aabb,
    on_ground: bool,
    age: i32,
    max_age: i32,
    scale: f32,
    gravity: f32,
    rgb: [f32; 3],
    layer: u32,
    jitter: (f32, f32),
    /// `ParticleDigging.sourcePos`: lit from here when its own block is dark.
    source: (i32, i32, i32),
}

#[derive(Resource)]
pub struct Particles {
    list: Vec<Particle>,
    /// `Math.random()` and the particle's own `Random`, one stream here.
    rand: JavaRandom,
}

impl Default for Particles {
    fn default() -> Self {
        Particles { list: Vec::new(), rand: JavaRandom::new(0x2545F491) }
    }
}

/// The particle mesh entity.
#[derive(Component)]
pub struct ParticleMesh;

/// `BlockColors.colorMultiplier` at one block, from the biome stored for its column.
fn tint(w: &Streamer, colors: &crate::colors::Colors, s: u16, x: i32, y: i32, z: i32) -> i32 {
    let biome = || biome_of(w.biome_at(x, z).unwrap_or(1));
    match s >> 4 {
        id::TALLGRASS | id::REEDS => colors.biome_grass(biome(), x, y, z),
        id::WATER | id::FLOWING_WATER => water_color(biome()),
        id::WATERLILY => 2129968,
        id::LEAVES => match s & 3 {
            1 => 6396257,
            2 => 8431445,
            _ => colors.biome_foliage(biome(), x, y, z),
        },
        id::LEAVES2 | id::VINE => colors.biome_foliage(biome(), x, y, z),
        id::DOUBLE_PLANT if matches!(s & 7, 2 | 3) => colors.biome_grass(biome(), x, y, z),
        _ => -1,
    }
}

impl Particles {
    fn random(&mut self) -> f64 {
        self.rand.next_double()
    }

    /// `new ParticleDigging(world, x, y, z, dx, dy, dz, state)`, then `setBlockPos(pos)` (or
    /// `init()` at its own block when `source` is None).
    #[allow(clippy::too_many_arguments)]
    fn digging(&mut self, atlas: &Atlas, colors: &crate::colors::Colors, w: &Streamer, at: DVec3, speed: DVec3, s: u16, source: Option<(i32, i32, i32)>) {
        let Some(sprite) = atlas.0.particle(s) else { return };
        if self.list.len() >= MAX {
            self.list.remove(0);
        }
        // Particle(World, x, y, z): size 0.2, jitter, scale and life.
        let jitter = (self.rand.next_float() * 3.0, self.rand.next_float() * 3.0);
        let scale = (self.rand.next_float() * 0.5 + 0.5) * 2.0;
        let max_age = (4.0 / (self.rand.next_float() * 0.9 + 0.1)) as i32;
        // Particle(World, x, y, z, xSpeed, ySpeed, zSpeed)
        let mut m = DVec3::new(
            speed.x + (self.random() * 2.0 - 1.0) * 0.4000000059604645,
            speed.y + (self.random() * 2.0 - 1.0) * 0.4000000059604645,
            speed.z + (self.random() * 2.0 - 1.0) * 0.4000000059604645,
        );
        let f = ((self.random() + self.random() + 1.0) as f32 * 0.15) as f64;
        let f1 = (m.length() as f32) as f64;
        m = m / f1 * f * 0.4000000059604645;
        m.y += 0.10000000149011612;
        let src = source.unwrap_or((at.x.floor() as i32, at.y.floor() as i32, at.z.floor() as i32));
        let mut rgb = [0.6f32; 3];
        // ParticleDigging.multiplyColor, which the grass block skips.
        if s >> 4 != id::GRASS {
            let c = tint(w, colors, s, src.0, src.1, src.2);
            if c != -1 {
                rgb = [0.6 * ((c >> 16) & 255) as f32 / 255.0, 0.6 * ((c >> 8) & 255) as f32 / 255.0, 0.6 * (c & 255) as f32 / 255.0];
            }
        }
        self.list.push(Particle {
            prev: at,
            pos: at,
            motion: m,
            bb: Aabb::new(at.x - 0.1, at.y, at.z - 0.1, at.x + 0.1, at.y + 0.2, at.z + 0.1),
            on_ground: false,
            age: 0,
            max_age,
            scale: scale / 2.0,
            gravity: PARTICLE_GRAVITY[(s >> 4) as usize & 255],
            rgb,
            layer: sprite.layer,
            jitter,
            source: src,
        });
    }

    /// `ParticleManager.addBlockDestroyEffects`: 4 x 4 x 4 debris.
    pub fn block_destroyed(&mut self, atlas: &Atlas, colors: &crate::colors::Colors, w: &Streamer, p: (i32, i32, i32), s: u16) {
        if mat(s) == material::AIR {
            return;
        }
        for j in 0..4 {
            for k in 0..4 {
                for l in 0..4 {
                    let d = DVec3::new((j as f64 + 0.5) / 4.0, (k as f64 + 0.5) / 4.0, (l as f64 + 0.5) / 4.0);
                    let at = DVec3::new(p.0 as f64, p.1 as f64, p.2 as f64) + d;
                    self.digging(atlas, colors, w, at, d - DVec3::splat(0.5), s, Some(p));
                }
            }
        }
    }

    /// `Entity.createRunningParticles` for a sprinting player at `feet` moving by `motion`.
    pub fn running(&mut self, atlas: &Atlas, colors: &crate::colors::Colors, w: &Streamer, feet: DVec3, motion: DVec3, width: f64) {
        let (x, y, z) = (feet.x.floor() as i32, (feet.y - 0.20000000298023224).floor() as i32, feet.z.floor() as i32);
        let s = w.state(x, y, z);
        if s == 0 || mat(s) == material::AIR {
            return;
        }
        let px = feet.x + (self.rand.next_float() as f64 - 0.5) * width;
        let pz = feet.z + (self.rand.next_float() as f64 - 0.5) * width;
        self.digging(atlas, colors, w, DVec3::new(px, feet.y + 0.1, pz), DVec3::new(-motion.x * 4.0, 1.5, -motion.z * 4.0), s, None);
    }

    /// `Particle.onUpdate` and `move` for every particle.
    fn tick(&mut self, w: &impl BlockAccess) {
        self.list.retain_mut(|p| {
            p.prev = p.pos;
            let alive = p.age < p.max_age;
            p.age += 1;
            if !alive {
                return false;
            }
            p.motion.y -= 0.04 * p.gravity as f64;
            let (mut x, mut y, mut z) = (p.motion.x, p.motion.y, p.motion.z);
            let y0 = y;
            let boxes = get_collision_boxes(w, &p.bb.expand(x, y, z));
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
            // The decompile reads `y != y`; the original compares against the asked motion.
            p.on_ground = y0 != y && y0 < 0.0;
            if x != p.motion.x {
                p.motion.x = 0.0;
            }
            if z != p.motion.z {
                p.motion.z = 0.0;
            }
            p.motion *= 0.9800000190734863;
            if p.on_ground {
                p.motion.x *= 0.699999988079071;
                p.motion.z *= 0.699999988079071;
            }
            true
        });
    }
}

pub fn spawn_mesh(mut commands: Commands, mut meshes: ResMut<Assets<Mesh>>, materials: Res<ChunkMaterials>) {
    // One degenerate quad: the mesh allocator refuses an empty mesh.
    let mut first = LayerMesh::default();
    first.quad([[8.0, 0.0, 8.0]; 4], [[0.0; 2]; 4], [0; 4], [[0; 3]; 4], [0.0, 1.0, 0.0], 0);
    commands.spawn((
        ParticleMesh,
        Mesh3d(meshes.add(vertex::to_mesh(first).0)),
        MeshMaterial3d(materials.cutout.clone()),
        Transform::default(),
        Visibility::Hidden,
        NoFrustumCulling,
        NotShadowCaster,
    ));
}

pub fn tick_particles(clock: Res<Clock>, streamer: Res<Streamer>, mut particles: ResMut<Particles>) {
    for _ in 0..clock.ticks_this_frame {
        particles.tick(&*streamer);
    }
}

/// `ParticleDigging.renderParticle` for every particle, into one mesh at the camera's chunk.
pub fn draw_particles(
    clock: Res<Clock>,
    view: Res<View>,
    streamer: Res<Streamer>,
    particles: Res<Particles>,
    mut meshes: ResMut<Assets<Mesh>>,
    entity: Single<(&Mesh3d, &mut Transform, &mut Visibility), With<ParticleMesh>>,
) {
    let (mesh, mut transform, mut visibility) = entity.into_inner();
    if particles.list.is_empty() {
        *visibility = Visibility::Hidden;
        return;
    }
    let pt = clock.partial as f64;
    let (cx, cz) = ((view.eye.x.floor() as i32) >> 4, (view.eye.z.floor() as i32) >> 4);
    let origin = DVec3::new((cx * 16) as f64, 0.0, (cz * 16) as f64);
    // ActiveRenderInfo's billboard axes.
    let (yaw, pitch) = (view.yaw.to_radians(), view.pitch.to_radians());
    let right = Vec3::new(yaw.cos(), 0.0, yaw.sin());
    let up = Vec3::new(-yaw.sin() * pitch.sin(), pitch.cos(), yaw.cos() * pitch.sin());
    let normal = -view.look;
    let mut out = LayerMesh::default();
    for p in &particles.list {
        let at = p.prev + (p.pos - p.prev) * pt;
        let local = (at - origin).as_vec3();
        // The packed vertex reaches 16 blocks around the chunk's center.
        if (local.x - 8.0).abs() > 15.5 || (local.z - 8.0).abs() > 15.5 || !(0.0..255.0).contains(&local.y) {
            continue;
        }
        // getBrightnessForRender, falling back to the source block.
        let light = |x: i32, y: i32, z: i32| -> Option<(u8, u8)> { streamer.light_at(x, y.clamp(0, 255), z) };
        let (sky, block) = light(at.x.floor() as i32, at.y.floor() as i32, at.z.floor() as i32)
            .filter(|&(s, b)| s > 0 || b > 0)
            .or_else(|| light(p.source.0, p.source.1, p.source.2))
            .unwrap_or((0, 0));
        let lm = (block as i32 * 16) | (sky as i32 * 16) << 16;
        let s = 0.1 * p.scale;
        let (r, u) = (right * s, up * s);
        let (u0, u1) = (p.jitter.0 / 4.0, (p.jitter.0 + 1.0) / 4.0);
        let (v0, v1) = (p.jitter.1 / 4.0, (p.jitter.1 + 1.0) / 4.0);
        let c = [(p.rgb[0] * 255.0) as u8, (p.rgb[1] * 255.0) as u8, (p.rgb[2] * 255.0) as u8];
        let pos = [local - r - u, local - r + u, local + r + u, local + r - u].map(|v| v.to_array());
        out.quad(pos, [[u0, v1], [u0, v0], [u1, v0], [u1, v1]], [lm; 4], [c; 4], normal.to_array(), p.layer | FLAG_NOMIP);
    }
    if out.is_empty() {
        *visibility = Visibility::Hidden;
        return;
    }
    *visibility = Visibility::Inherited;
    *transform = Transform::from_xyz(origin.x as f32 + 8.0, 0.0, origin.z as f32 + 8.0);
    let _ = meshes.insert(&mesh.0, vertex::to_mesh(out).0);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debris_falls_lands_and_expires() {
        let mut p = Particles::default();
        let at = DVec3::new(0.5, 1.5, 0.5);
        p.list.push(Particle {
            prev: at,
            pos: at,
            motion: DVec3::new(0.1, 0.0, 0.0),
            bb: Aabb::new(0.4, 1.5, 0.4, 0.6, 1.7, 0.6),
            on_ground: false,
            age: 0,
            max_age: 20,
            scale: 1.0,
            gravity: 1.0,
            rgb: [0.6; 3],
            layer: 0,
            jitter: (0.0, 0.0),
            source: (0, 1, 0),
        });
        // A floor whose top is y 1.
        let w = crate::collision::tests::Blocks::floor_at(0, 4);
        let mut ticks = 0;
        while !p.list.is_empty() {
            p.tick(&w);
            ticks += 1;
        }
        assert_eq!(ticks, 21, "alive for ages 0 to 19, gone on the 21st tick");
        let mut p2 = Particles::default();
        p2.list.push(Particle { max_age: 100, ..p_first(at) });
        for _ in 0..40 {
            p2.tick(&w);
        }
        let q = &p2.list[0];
        assert!((q.pos.y - 1.0).abs() < 1e-9 && q.on_ground, "rests on the floor: {}", q.pos.y);
    }

    fn p_first(at: DVec3) -> Particle {
        Particle {
            prev: at,
            pos: at,
            motion: DVec3::ZERO,
            bb: Aabb::new(at.x - 0.1, at.y, at.z - 0.1, at.x + 0.1, at.y + 0.2, at.z + 0.1),
            on_ground: false,
            age: 0,
            max_age: 1,
            scale: 1.0,
            gravity: 1.0,
            rgb: [0.6; 3],
            layer: 0,
            jitter: (0.0, 0.0),
            source: (0, 1, 0),
        }
    }
}
