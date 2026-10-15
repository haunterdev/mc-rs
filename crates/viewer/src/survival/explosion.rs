//! `S_EXPLOSION`: `NetHandlerPlayClient.handleExplosion`, which runs `Explosion.doExplosionB(true)`
//! (the `entity.generic.explode` sound, volume 4, pitch `(1 + (r1 - r2) * 0.2) * 0.7`, and the
//! explosion particle) and adds the packet's motion to the own player. The block removal arrives as
//! `S_BLOCKS`.
//!
//! The particle is `EXPLOSION_HUGE` when the size is 2 or more (`damagesTerrain` is always true
//! for a packet), else `EXPLOSION_LARGE`. [`Large`] is `ParticleExplosionLarge`: 6 to 9 ticks,
//! frame `(life + partial) * 15 / lifeTime` of `textures/entity/explosion.png` (4 by 4 cells), half
//! size `2 * (1 - xSpeed * 0.5)`, grey 0.4 to 1. [`Huge`] is `ParticleExplosionHuge`: eight ticks,
//! each tick six larges within 4 blocks of the centre with `xSpeed = tick / 8`.
//!
//! DROPPED: the block break particles of `doExplosionB` (the affected block list is not on the
//! wire), the `ParticleManager` distance check, and the lightmap (the particle is full bright).

use bevy::asset::RenderAssetUsages;
use bevy::camera::visibility::NoFrustumCulling;
use bevy::light::NotShadowCaster;
use bevy::math::DVec3;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use worldgen::JavaRandom;

use super::Explosion as ExplosionMsg;
use crate::audio::{Audio, Category};
use crate::player::Player;
use crate::sky::{Clock, View};

/// `textures/entity/explosion.png`, loaded at startup (main.rs).
#[derive(Resource)]
pub struct ExplosionImage(pub Handle<Image>);

/// Which particle `doExplosionB` spawns for a size.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Huge,
    Large,
}

pub fn kind_of(size: f32) -> Kind {
    if size >= 2.0 {
        Kind::Huge
    } else {
        Kind::Large
    }
}

/// The sound's pitch: `(1 + (r1 - r2) * 0.2) * 0.7`.
pub fn pitch_of(r1: f32, r2: f32) -> f32 {
    (1.0 + (r1 - r2) * 0.2) * 0.7
}

#[derive(Clone, Debug)]
pub struct Large {
    pub pos: DVec3,
    pub life: i32,
    pub life_time: i32,
    pub size: f32,
    pub grey: f32,
}

impl Large {
    /// `ParticleExplosionLarge.renderParticle`'s frame; None once past the 16 cells.
    pub fn frame(&self, partial: f32) -> Option<i32> {
        let i = ((self.life as f32 + partial) * 15.0 / self.life_time as f32) as i32;
        (i <= 15).then_some(i)
    }
}

#[derive(Clone, Debug)]
pub struct Huge {
    pub pos: DVec3,
    pub time: i32,
}

#[derive(Resource)]
pub struct Explosions {
    pub larges: Vec<Large>,
    pub huges: Vec<Huge>,
    /// Particles added this tick join the list at the next one (`ParticleManager.queue`).
    queue: Vec<Large>,
    rand: JavaRandom,
    mesh: Option<(Handle<Mesh>, Entity)>,
}

impl Default for Explosions {
    fn default() -> Explosions {
        Explosions { larges: Vec::new(), huges: Vec::new(), queue: Vec::new(), rand: JavaRandom::new(0x2B7E1516), mesh: None }
    }
}

impl Explosions {
    /// `new ParticleExplosionLarge(.., xSpeed, ..)`
    pub fn spawn_large(&mut self, pos: DVec3, x_speed: f64) {
        let life_time = 6 + self.rand.next_int_bounded(4);
        let f = self.rand.next_float() * 0.6 + 0.4;
        self.queue.push(Large { pos, life: 0, life_time, size: 1.0 - x_speed as f32 * 0.5, grey: f });
    }

    /// `World.spawnParticle(EXPLOSION_HUGE | EXPLOSION_LARGE, x, y, z, 1, 0, 0)`.
    pub fn explode(&mut self, pos: DVec3, size: f32) {
        match kind_of(size) {
            Kind::Huge => self.huges.push(Huge { pos, time: 0 }),
            Kind::Large => self.spawn_large(pos, 1.0),
        }
    }

    /// One client tick of `ParticleManager.updateEffects`.
    pub fn tick(&mut self) {
        self.larges.append(&mut self.queue);
        for p in &mut self.larges {
            p.life += 1;
        }
        self.larges.retain(|p| p.life < p.life_time);
        let mut spawn = Vec::new();
        for h in &mut self.huges {
            for _ in 0..6 {
                let r = &mut self.rand;
                let d0 = h.pos.x + (r.next_double() - r.next_double()) * 4.0;
                let d1 = h.pos.y + (r.next_double() - r.next_double()) * 4.0;
                let d2 = h.pos.z + (r.next_double() - r.next_double()) * 4.0;
                spawn.push((DVec3::new(d0, d1, d2), (h.time as f32 / 8.0) as f64));
            }
            h.time += 1;
        }
        self.huges.retain(|h| h.time < 8);
        for (p, s) in spawn {
            self.spawn_large(p, s);
        }
    }
}

fn frames(mut msgs: MessageReader<ExplosionMsg>, audio: Option<Res<Audio>>, mut fx: ResMut<Explosions>, player: Option<ResMut<Player>>) {
    let mut player = player;
    for ExplosionMsg(e) in msgs.read() {
        let at = DVec3::new(e.pos[0], e.pos[1], e.pos[2]);
        if let Some(audio) = &audio {
            let pitch = pitch_of(audio.next_float(), audio.next_float());
            audio.play("entity.generic.explode", Category::Blocks, Some(at.as_vec3()), 4.0, pitch);
        }
        fx.explode(at, e.strength);
        // handleExplosion: motionX += packet.getMotionX()
        if let Some(p) = player.as_mut() {
            p.body.motion += DVec3::new(e.motion[0] as f64, e.motion[1] as f64, e.motion[2] as f64);
        }
    }
}

fn tick(clock: Res<Clock>, mut fx: ResMut<Explosions>) {
    for _ in 0..clock.ticks_this_frame {
        fx.tick();
    }
}

/// The quads of every large particle, in blocks from `origin`.
fn build(list: &[Large], right: Vec3, up: Vec3, partial: f32, origin: DVec3) -> Mesh {
    let (mut pos, mut uv, mut col, mut idx) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    let mut quad = |corners: [Vec3; 4], uvs: [[f32; 2]; 4], c: [f32; 4]| {
        let base = pos.len() as u32;
        pos.extend(corners.map(|v: Vec3| v.to_array()));
        uv.extend(uvs);
        col.extend([c; 4]);
        idx.extend([base, base + 1, base + 2, base, base + 2, base + 3]);
    };
    let mut any = false;
    for p in list {
        let Some(i) = p.frame(partial) else { continue };
        any = true;
        let (u0, v0) = ((i % 4) as f32 / 4.0, (i / 4) as f32 / 4.0);
        let (u1, v1) = (u0 + 0.249_75, v0 + 0.249_75);
        let s = 2.0 * p.size;
        let (r, u) = (right * s, up * s);
        let at = (p.pos - origin).as_vec3();
        let c = Color::srgb(p.grey, p.grey, p.grey).to_linear();
        quad([at - r - u, at - r + u, at + r + u, at + r - u], [[u0, v1], [u0, v0], [u1, v0], [u1, v1]], [c.red, c.green, c.blue, 1.0]);
    }
    if !any {
        quad([Vec3::ZERO; 4], [[0.0; 2]; 4], [1.0; 4]);
    }
    let n = pos.len();
    Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, pos)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.0, 1.0, 0.0]; n])
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uv)
        .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, col)
        .with_inserted_indices(Indices::U32(idx))
}

fn draw(
    mut commands: Commands,
    clock: Res<Clock>,
    view: Res<View>,
    image: Option<Res<ExplosionImage>>,
    mut fx: ResMut<Explosions>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut shown: Query<(&mut Transform, &mut Visibility)>,
) {
    let Some(image) = image else { return };
    if fx.mesh.is_none() {
        if fx.larges.is_empty() {
            return;
        }
        let mesh = meshes.add(build(&[], Vec3::X, Vec3::Y, 0.0, DVec3::ZERO));
        let e = commands
            .spawn((
                Mesh3d(mesh.clone()),
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
            ))
            .id();
        fx.mesh = Some((mesh, e));
    }
    let Some((mesh, e)) = fx.mesh.clone() else { return };
    let Ok((mut transform, mut visibility)) = shown.get_mut(e) else { return };
    if fx.larges.is_empty() {
        *visibility = Visibility::Hidden;
        return;
    }
    // ActiveRenderInfo's billboard axes.
    let (yaw, pitch) = (view.yaw.to_radians(), view.pitch.to_radians());
    let right = Vec3::new(yaw.cos(), 0.0, yaw.sin());
    let up = Vec3::new(-yaw.sin() * pitch.sin(), pitch.cos(), yaw.cos() * pitch.sin());
    let origin = DVec3::new((view.eye.x.floor() as i32 & !15) as f64, 0.0, (view.eye.z.floor() as i32 & !15) as f64);
    let _ = meshes.insert(&mesh, build(&fx.larges, right, up, clock.partial, origin));
    *transform = Transform::from_xyz(origin.x as f32, 0.0, origin.z as f32);
    *visibility = Visibility::Inherited;
}

pub fn plugin(app: &mut App) {
    app.init_resource::<Explosions>().add_systems(
        Update,
        (
            frames,
            tick.after(crate::player::run_ticks).run_if(resource_exists::<Clock>),
            draw.after(crate::sky::update_view).after(tick).run_if(resource_exists::<Clock>).run_if(resource_exists::<View>),
        ),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn size_two_or_more_is_the_huge_emitter() {
        assert_eq!(kind_of(4.0), Kind::Huge);
        assert_eq!(kind_of(2.0), Kind::Huge);
        assert_eq!(kind_of(1.9), Kind::Large);
        assert_eq!(kind_of(0.0), Kind::Large);
        let mut fx = Explosions::default();
        fx.explode(DVec3::new(1.0, 64.0, 1.0), 4.0);
        assert_eq!((fx.huges.len(), fx.larges.len() + fx.queue.len()), (1, 0));
        fx.explode(DVec3::new(1.0, 64.0, 1.0), 1.0);
        assert_eq!((fx.huges.len(), fx.queue.len()), (1, 1));
    }

    #[test]
    fn the_sound_pitch_spreads_around_0_7() {
        assert!((pitch_of(0.5, 0.5) - 0.7).abs() < 1.0e-6);
        assert!((pitch_of(1.0, 0.0) - 0.84).abs() < 1.0e-6);
        assert!((pitch_of(0.0, 1.0) - 0.56).abs() < 1.0e-6);
    }

    #[test]
    fn a_huge_emitter_throws_six_larges_a_tick_for_eight_ticks() {
        let mut fx = Explosions::default();
        fx.explode(DVec3::new(0.0, 64.0, 0.0), 4.0);
        for t in 0..8 {
            fx.tick();
            assert_eq!(fx.queue.len() >= 6, true, "tick {t}");
            for p in &fx.queue {
                assert!(p.pos.x.abs() <= 4.0 && p.pos.y > 60.0 && p.pos.y < 68.0);
            }
        }
        assert!(fx.huges.is_empty(), "expired after eight updates");
    }

    #[test]
    fn a_large_lives_six_to_nine_ticks_and_steps_through_sixteen_frames() {
        let mut fx = Explosions::default();
        fx.explode(DVec3::ZERO, 1.0);
        fx.tick();
        let p = fx.larges[0].clone();
        assert!((6..=9).contains(&p.life_time) && p.grey >= 0.4 && p.grey <= 1.0);
        assert_eq!(p.size, 0.5, "1 - xSpeed * 0.5");
        assert_eq!(Large { life: 0, ..p.clone() }.frame(0.0), Some(0));
        assert!(Large { life: p.life_time, ..p.clone() }.frame(1.0).is_none());
        let mut n = 1;
        while !fx.larges.is_empty() {
            fx.tick();
            n += 1;
        }
        assert!(n >= 6 && n <= 9, "{n}");
    }
}
