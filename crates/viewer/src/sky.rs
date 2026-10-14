//! Time of day, the view and the sky: the clock `Minecraft.runTick` advances, what
//! `EntityRenderer` derives from the view entity (fog color and brightness, field of view, the
//! environment texture), the shaded mode's sun, and `RenderGlobal.renderSky` as one mesh the sky
//! camera draws before anything else.

use std::sync::Arc;

use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::math::DVec3;
use bevy::prelude::*;
use worldgen::math_helper::{cos, sin};
use worldgen::JavaRandom;

use crate::celestial::{self, Fluid, FogInput};
use crate::colors::{biome_of, Colors};
use crate::env::{self, Env};
use crate::lightmap;
use crate::player::{self, Player};
use crate::region::{props, Mat};
use crate::settings::{Mode, Settings};
use crate::stream::Streamer;
use crate::SkyCamera;

/// `Minecraft.timer` and the per-tick counters the renderer reads.
#[derive(Resource)]
pub struct Clock {
    /// `World.getWorldTime`
    pub world_time: i64,
    /// `Timer.renderPartialTicks`
    pub partial: f32,
    /// Client ticks run so far, the clock `TextureMap.updateAnimations` steps with.
    pub ticks: u64,
    /// `RenderGlobal.cloudTickCounter`
    pub cloud_ticks: i32,
    /// `World.skylightSubtracted`
    pub skylight_subtracted: i32,
    /// Ticks `advance_clock` ran this frame, for the systems that tick after it.
    pub ticks_this_frame: u32,
    /// The server's clock in a shared world (`proto::S_TIME`); `None` playing alone.
    pub server: Option<ServerClock>,
}

/// The shared world's clock as the server last sent it.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ServerClock {
    /// `doDaylightCycle`
    pub cycle: bool,
    /// Ticks the local clock is behind the server's (ahead when negative), made up one tick per
    /// tick so the sun never jumps.
    pub behind: i64,
}

/// A difference past this many ticks (5 seconds) jumps instead of easing.
pub const SNAP_TICKS: i64 = 100;

impl Clock {
    pub fn new(world_time: i64) -> Clock {
        let skylight_subtracted = celestial::skylight_subtracted(celestial::celestial_angle(world_time, 1.0));
        Clock { world_time, partial: 0.0, ticks: 0, cloud_ticks: 0, skylight_subtracted, ticks_this_frame: 0, server: None }
    }

    /// The server's time: jump to it when told to or when far off, else ease toward it. From the
    /// first sync on, the server's clock runs this one and the options' fixed time is ignored.
    pub fn sync(&mut self, t: proto::TimeSync) {
        let d = t.world_time - self.world_time;
        let behind = if self.server.is_none() || t.snap || d.abs() > SNAP_TICKS {
            self.world_time = t.world_time;
            self.skylight_subtracted = celestial::skylight_subtracted(celestial::celestial_angle(self.world_time, 1.0));
            0
        } else {
            d
        };
        self.server = Some(ServerClock { cycle: t.cycle, behind });
    }

    /// What one tick adds to the time: one with the cycle on, give or take one to catch up.
    fn step(&mut self) -> i64 {
        match &mut self.server {
            None => 1,
            Some(s) => {
                let catch = s.behind.signum();
                s.behind -= catch;
                s.cycle as i64 + catch
            }
        }
    }

    /// `World.getCelestialAngle(partialTicks)`
    pub fn angle(&self) -> f32 {
        celestial::celestial_angle(self.world_time, self.partial)
    }
}

/// The view entity as the renderer sees it this frame.
#[derive(Resource)]
pub struct View {
    /// The camera position, `ActiveRenderInfo.getPosition` in world space.
    pub eye: Vec3,
    /// The view entity's interpolated position.
    pub feet: Vec3,
    /// `Entity.getEyeHeight`
    pub eye_height: f32,
    /// `Entity.getLook`
    pub look: Vec3,
    /// `rotationPitch`: degrees, positive looking down.
    pub pitch: f32,
    /// `rotationYaw`: degrees, unbounded like vanilla's so the arm sway never sees a wrap.
    pub yaw: f32,
    /// The fluid `ActiveRenderInfo.getBlockStateAtEntityViewpoint` finds at the eye.
    pub fluid: Fluid,
    /// `World.getSkyColor` at the view entity.
    pub sky: [f32; 3],
    /// `EntityRenderer.updateFogColor`, which is also the clear color.
    pub fog: [f32; 3],
    /// `EntityRenderer.fogColor2`, `fogColor1`: the fog brightness, smoothed per tick.
    pub fog_color2: f32,
    pub fog_color1: f32,
    /// `EntityRenderer.fovModifierHandPrev`, `fovModifierHand`
    pub fov_hand_prev: f32,
    pub fov_hand: f32,
    /// `EntityRenderer.applyBobbing`, identity with view bobbing off.
    pub bob: Mat4,
    /// The main camera's world transform: the inverse of `setupCameraTransform`'s modelview.
    pub camera: Mat4,
    /// `GameSettings.thirdPersonView`: 0 first person, 1 behind, 2 in front looking back (F5).
    pub third_person: u8,
}

impl Default for View {
    fn default() -> View {
        View {
            third_person: 0,
            eye: Vec3::ZERO,
            feet: Vec3::ZERO,
            eye_height: 1.62,
            look: Vec3::Z,
            pitch: 0.0,
            yaw: 0.0,
            fluid: Fluid::None,
            sky: [0.0; 3],
            fog: [0.0; 3],
            fog_color2: 1.0,
            fog_color1: 1.0,
            // The walking value, so the first frames do not zoom in from `fovModifierHand = 0`.
            fov_hand_prev: 1.0,
            fov_hand: 1.0,
            bob: Mat4::IDENTITY,
            camera: Mat4::IDENTITY,
        }
    }
}

impl View {
    /// `EntityRenderer.getFOVModifier(partialTicks, true)`
    pub fn fov(&self, settings: &Settings, partial: f32) -> f32 {
        let mut f = settings.fov * (self.fov_hand_prev + (self.fov_hand - self.fov_hand_prev) * partial);
        // DROPPED: the death zoom; the view entity never dies.
        if self.fluid == Fluid::Water {
            f = f * 60.0 / 70.0;
        }
        f
    }
}

/// Biome colors and temperatures, shared with the chunk workers.
#[derive(Resource)]
pub struct BiomeColors(pub Arc<Colors>);

/// `ActiveRenderInfo.getBlockStateAtEntityViewpoint`, reduced to the fluid it finds.
pub fn fluid_at(s: &Streamer, eye: Vec3) -> Fluid {
    let (x, y, z) = (eye.x.floor() as i32, eye.y.floor() as i32, eye.z.floor() as i32);
    let mut state = s.state_at(x, y, z).unwrap_or(0);
    if props(state).liquid {
        let f = celestial::liquid_height_percent((state & 15) as i32) - 0.111_111_11;
        let f1 = (y + 1) as f32 - f;
        if eye.y >= f1 {
            state = s.state_at(x, y + 1, z).unwrap_or(0);
        }
    }
    match props(state).mat {
        Mat::Water => Fluid::Water,
        Mat::Lava => Fluid::Lava,
        _ => Fluid::None,
    }
}

/// `Chunk.getLightSubtracted` through `World.getLight(pos, false)`. An unloaded chunk is the
/// client's `EmptyChunk`, which answers 0.
fn light_subtracted(s: &Streamer, x: i32, y: i32, z: i32, amount: i32) -> i32 {
    if y < 0 {
        return 0;
    }
    match s.light_at(x, y.min(255), z) {
        Some((sky, block)) => (sky as i32 - amount).max(block as i32),
        None => 0,
    }
}

/// `World.getLight(pos, true)`
pub fn world_light(s: &Streamer, x: i32, y: i32, z: i32, amount: i32) -> i32 {
    if props(s.state_at(x, y, z).unwrap_or(0)).use_neighbor_brightness() {
        [(0, 1, 0), (1, 0, 0), (-1, 0, 0), (0, 0, 1), (0, 0, -1)]
            .iter()
            .map(|d| light_subtracted(s, x + d.0, y + d.1, z + d.2, amount))
            .max()
            .unwrap_or(0)
    } else {
        light_subtracted(s, x, y, z, amount)
    }
}

/// `World.getLightFromNeighborsFor` for both light types, as `getCombinedLight(pos, 0)` reads
/// them: `(sky, block)`. Invalid and unloaded positions read the defaults, sky 15 and block 0.
pub fn neighbor_light(s: &Streamer, x: i32, y: i32, z: i32) -> (u8, u8) {
    let get = |x: i32, y: i32, z: i32| -> (u8, u8) {
        let y = y.max(0);
        if y >= 256 {
            return (15, 0);
        }
        s.light_at(x, y, z).unwrap_or((15, 0))
    };
    let y = y.max(0);
    if y >= 256 || s.info_at(x, z).is_none() {
        return (15, 0);
    }
    if props(s.state_at(x, y, z).unwrap_or(0)).use_neighbor_brightness() {
        let n = [(0, 1, 0), (1, 0, 0), (-1, 0, 0), (0, 0, 1), (0, 0, -1)].map(|d| get(x + d.0, y + d.1, z + d.2));
        (n.iter().map(|l| l.0).max().unwrap_or(0), n.iter().map(|l| l.1).max().unwrap_or(0))
    } else {
        get(x, y, z)
    }
}

/// `Timer.updateTimer` and the world's share of `Minecraft.runTick`: at most ten 50 ms ticks a
/// frame, each stepping the cloud counter, the skylight and, with the daylight cycle on, the time.
pub fn advance_clock(time: Res<Time>, settings: Res<Settings>, mut clock: ResMut<Clock>) {
    clock.partial += time.delta_secs() * 20.0;
    let elapsed = clock.partial as i32;
    clock.partial -= elapsed as f32;
    let n = elapsed.min(10).max(0) as u32;
    clock.ticks_this_frame = n;
    let fixed = settings.time.ticks().filter(|_| clock.server.is_none());
    if let Some(t) = fixed {
        clock.world_time = t;
    }
    for _ in 0..n {
        clock.ticks += 1;
        // RenderGlobal.updateClouds
        clock.cloud_ticks += 1;
        // World.tick: the skylight from the time before this tick's step.
        clock.skylight_subtracted = celestial::skylight_subtracted(celestial::celestial_angle(clock.world_time, 1.0));
        // WorldClient.tick with doDaylightCycle.
        if fixed.is_none() {
            clock.world_time += clock.step();
        }
    }
}

/// One tick of `EntityRenderer.updateRenderer`: the FOV modifier and the fog brightness, read
/// from the player before its own update.
pub fn renderer_tick(view: &mut View, player: &Player, streamer: &Streamer, clock: &Clock, settings: &Settings) {
    // updateFovModifierHand
    let modifier = player.fov_modifier();
    view.fov_hand_prev = view.fov_hand;
    view.fov_hand += (modifier - view.fov_hand) * 0.5;
    view.fov_hand = view.fov_hand.clamp(0.1, 1.5);

    // The light at BlockPos(entity), smoothed. The blend weight is renderDistanceChunks / 32,
    // which the game never takes past 32.
    let p = player.pos;
    let (bx, by, bz) = (p.x.floor() as i32, p.y.floor() as i32, p.z.floor() as i32);
    let light = world_light(streamer, bx, by, bz, clock.skylight_subtracted);
    let brightness = lightmap::brightness_table()[light.clamp(0, 15) as usize];
    let rd = settings.render_distance.min(32) as u32;
    (view.fog_color2, view.fog_color1) = celestial::fog_brightness_tick(view.fog_color1, brightness, rd);
}

/// Reads the player into `View` for this frame: the interpolated position and angles, the
/// camera (`setupCameraTransform` with `applyBobbing` and `orientCamera`), the fluid at the
/// camera and the sky color at the feet.
pub fn update_view(
    clock: Res<Clock>,
    settings: Res<Settings>,
    streamer: Res<Streamer>,
    colors: Res<BiomeColors>,
    player: Res<Player>,
    mut view: ResMut<View>,
) {
    let pt = clock.partial;
    view.feet = player.interp_pos(pt).as_vec3();
    view.eye_height = player.eye_height();
    view.pitch = player.interp_pitch(pt);
    view.yaw = player.interp_yaw(pt);
    view.look = player::look(view.pitch, view.yaw);
    view.bob = if settings.bob_view { player.bobbing(pt) } else { Mat4::IDENTITY };
    // The modelview is bob * orientCamera * Rx(pitch) * Ry(yaw + 180) * translate(0,
    // -eyeHeight, 0), relative to the feet; the camera is its inverse. orientCamera is
    // translate(0, 0, 0.05) in first person, translate(0, 0, -d) behind the player, and
    // rotate(180, Y) * translate(0, 0, d) in front of it.
    let eye = view.feet + Vec3::Y * view.eye_height;
    let orient = match view.third_person {
        0 => Mat4::from_translation(Vec3::new(0.0, 0.0, -0.05)),
        mode => {
            let d = third_person_distance(&streamer, eye.as_dvec3(), view.yaw, view.pitch, mode);
            if mode == 1 {
                Mat4::from_translation(Vec3::new(0.0, 0.0, d))
            } else {
                Mat4::from_translation(Vec3::new(0.0, 0.0, -d)) * Mat4::from_rotation_y(std::f32::consts::PI)
            }
        }
    };
    view.camera = Mat4::from_translation(eye)
        * Mat4::from_rotation_y(-(view.yaw + 180.0).to_radians())
        * Mat4::from_rotation_x(-view.pitch.to_radians())
        * orient
        * view.bob.inverse();
    view.eye = view.camera.w_axis.truncate();
    view.fluid = fluid_at(&streamer, view.eye);

    // World.getSkyColor: the biome and temperature at the feet's block.
    let (bx, by, bz) = (view.feet.x.floor() as i32, view.feet.y.floor() as i32, view.feet.z.floor() as i32);
    let biome = match streamer.biome_at(bx, bz) {
        Some(id) => biome_of(id),
        None => streamer.generator.provider.get_biomes(bx, bz, 1, 1)[0],
    };
    let temperature = colors.0.temperature(biome, bx, by, bz);
    view.sky = celestial::sky_color(clock.angle(), temperature);
}

/// `EntityRenderer.orientCamera`'s distance: 4 blocks, pulled in to the nearest of eight ray
/// traces from the corners of a 0.2 box around the eye.
/// DROPPED: `thirdPersonDistancePrev`, which is always 4 in 1.12.2.
pub fn third_person_distance(w: &Streamer, eye: DVec3, yaw: f32, pitch: f32, mode: u8) -> f32 {
    let mut d3 = 4.0f64;
    let f1 = yaw;
    let f2 = if mode == 2 { pitch + 180.0 } else { pitch };
    let (f1r, f2r) = ((f1 * 0.017453292) as f64, (f2 * 0.017453292) as f64);
    let d4 = -f1r.sin() * f2r.cos() * d3;
    let d5 = f1r.cos() * f2r.cos() * d3;
    let d6 = -f2r.sin() * d3;
    for i in 0..8 {
        let f3 = (((i & 1) * 2 - 1) as f32 * 0.1) as f64;
        let f4 = ((((i >> 1) & 1) * 2 - 1) as f32 * 0.1) as f64;
        let f5 = ((((i >> 2) & 1) * 2 - 1) as f32 * 0.1) as f64;
        let start = DVec3::new(eye.x + f3, eye.y + f4, eye.z + f5);
        let end = DVec3::new(eye.x - d4 + f3 + f5, eye.y - d6 + f4, eye.z - d5 + f5);
        if let Some(hit) = crate::target::ray_trace_blocks(w, start, end, false, false) {
            let d7 = hit.vec.distance(eye);
            if d7 < d3 {
                d3 = d7;
            }
        }
    }
    d3 as f32
}

/// The shaded mode's light: the sun by day, the moon by night, faded in over the horizon.
/// Returns the direction toward the light and its strength.
pub fn sun_light(angle: f32) -> (Vec3, f32) {
    let theta = celestial::celestial_angle_radians(angle);
    // The sun's quad sits at Ry(-90) Rx(angle * 360) (0, 100, 0).
    let sun = Vec3::new(-theta.sin(), theta.cos(), 0.0);
    let smooth = |e: f32| {
        let t = (e / 0.15).clamp(0.0, 1.0);
        t * t * (3.0 - 2.0 * t)
    };
    if sun.y >= 0.0 {
        (sun, 0.85 * smooth(sun.y))
    } else {
        (-sun, 0.12 * smooth(-sun.y))
    }
}

/// The fog, the lightmap, the shading mode and the clocks the shaders read, plus the clear color
/// (`EntityRenderer.updateFogColor`) and the shaded mode's directional light.
pub fn write_env(
    clock: Res<Clock>,
    settings: Res<Settings>,
    mut view: ResMut<View>,
    mut env: ResMut<Env>,
    mut sky_camera: Single<&mut Camera, With<SkyCamera>>,
    mut light: Single<(&mut Transform, &mut DirectionalLight)>,
) {
    let angle = clock.angle();
    let rd = settings.render_distance as u32;
    let input = FogInput {
        // updateFogColor's 0.25 + 0.75 * rd / 32 blend, at most the game's 32.
        render_distance: rd.min(32),
        sky: view.sky,
        look: view.look.to_array(),
        fluid: view.fluid,
        brightness: view.fog_color2 + (view.fog_color1 - view.fog_color2) * clock.partial,
        feet_y: view.feet.y as f64,
    };
    let fog = celestial::fog_color(angle, &input);
    view.fog = fog;
    sky_camera.clear_color = ClearColorConfig::Custom(Color::srgb(fog[0], fog[1], fog[2]));
    env.set(env::P_FOG_COLOR, [fog[0], fog[1], fog[2], 1.0]);

    // EntityRenderer.setupFog: exponential in a fluid, linear in the open, where the sky
    // (`startCoords` -1) starts at the eye and terrain and clouds at three quarters out.
    // DROPPED: water breathing and respiration; the view entity has neither.
    let fluid_fog = match view.fluid {
        Fluid::Water => Some(0.1),
        Fluid::Lava => Some(2.0),
        Fluid::None => None,
    };
    // Terrain fog reaches the distant terrain (lod.rs) when it is on; the sky's stays at the
    // render distance.
    let fog_params = |chunks: u32, start_coords: i32| match fluid_fog {
        Some(density) => [0.0, 0.0, density, 1.0],
        None => {
            let (s, e) = celestial::linear_fog(chunks, start_coords);
            [s, e, 0.0, 0.0]
        }
    };
    env.set(env::P_FOG, fog_params(settings.view_chunks() as u32, 0));
    env.set(env::P_SKY_FOG, fog_params(rd, -1));

    // EntityRenderer.updateLightmap reads the sun at partial tick 1.
    let sun = celestial::sun_brightness(celestial::celestial_angle(clock.world_time, 1.0));
    env.set_lightmap(&lightmap::lightmap(sun, settings.gamma));

    let (dir, strength) = sun_light(angle);
    let shaded = settings.mode == Mode::Shaded;
    let shadows = shaded && settings.shadows;
    env.set(env::P_SHADING, [shaded as u32 as f32, strength, 0.6, shadows as u32 as f32]);
    env.set(env::P_TIME, [clock.ticks as f32, clock.partial, 0.0, 0.0]);
    let c = celestial::cloud_colour(angle);
    env.set(env::P_CLOUD_COLOR, [c[0], c[1], c[2], 1.0]);

    let (transform, dl) = &mut *light;
    **transform = Transform::IDENTITY.looking_to(-dir, Vec3::Z);
    if dl.shadow_maps_enabled != shadows {
        dl.shadow_maps_enabled = shadows;
    }
}

/// Marks the sky mesh entity.
#[derive(Component)]
pub struct SkyMesh;

/// `RenderGlobal.renderStars`, built once.
#[derive(Resource)]
pub struct Stars(pub Vec<[[f32; 3]; 4]>);

/// `RenderGlobal.renderStars`: the star quads in the frame of the sun and moon.
pub fn stars() -> Vec<[[f32; 3]; 4]> {
    let mut random = JavaRandom::new(10842);
    let mut out = Vec::new();
    for _ in 0..1500 {
        let mut d0 = (random.next_float() * 2.0 - 1.0) as f64;
        let mut d1 = (random.next_float() * 2.0 - 1.0) as f64;
        let mut d2 = (random.next_float() * 2.0 - 1.0) as f64;
        let d3 = (0.15f32 + random.next_float() * 0.1) as f64;
        let mut d4 = d0 * d0 + d1 * d1 + d2 * d2;
        if d4 < 1.0 && d4 > 0.01 {
            d4 = 1.0 / d4.sqrt();
            d0 *= d4;
            d1 *= d4;
            d2 *= d4;
            let (d5, d6, d7) = (d0 * 100.0, d1 * 100.0, d2 * 100.0);
            let d8 = d0.atan2(d2);
            let (d9, d10) = (d8.sin(), d8.cos());
            let d11 = (d0 * d0 + d2 * d2).sqrt().atan2(d1);
            let (d12, d13) = (d11.sin(), d11.cos());
            let d14 = random.next_double() * std::f64::consts::PI * 2.0;
            let (d15, d16) = (d14.sin(), d14.cos());
            out.push(std::array::from_fn(|j| {
                let j = j as i32;
                let d18 = ((j & 2) - 1) as f64 * d3;
                let d19 = (((j + 1) & 2) - 1) as f64 * d3;
                let d21 = d18 * d16 - d19 * d15;
                let d22 = d19 * d16 + d18 * d15;
                let d23 = d21 * d12 + 0.0 * d13;
                let d24 = 0.0 * d12 - d21 * d13;
                let d25 = d24 * d9 - d22 * d10;
                let d26 = d22 * d9 + d24 * d10;
                [(d5 + d25) as f32, (d6 + d23) as f32, (d7 + d26) as f32]
            }));
        }
    }
    out
}

/// What the sky shader does with a vertex (sky.wgsl): the vertex's `uv_b.x`.
const K_PLANE: f32 = 0.0;
const K_FAN: f32 = 1.0;
const K_SUN: f32 = 2.0;
const K_MOON: f32 = 3.0;
const K_STAR: f32 = 4.0;

#[derive(Default)]
struct SkyBuilder {
    pos: Vec<[f32; 3]>,
    uv: Vec<[f32; 2]>,
    kind: Vec<[f32; 2]>,
    color: Vec<[f32; 4]>,
    idx: Vec<u32>,
}

impl SkyBuilder {
    fn vertex(&mut self, p: Vec3, uv: [f32; 2], kind: f32, c: [f32; 4]) -> u32 {
        self.pos.push(p.to_array());
        self.uv.push(uv);
        self.kind.push([kind, 0.0]);
        self.color.push(c);
        self.pos.len() as u32 - 1
    }

    fn quad(&mut self, p: [Vec3; 4], uv: [[f32; 2]; 4], kind: f32, c: [f32; 4]) {
        let i: [u32; 4] = std::array::from_fn(|k| self.vertex(p[k], uv[k], kind, c));
        self.idx.extend_from_slice(&[i[0], i[1], i[2], i[0], i[2], i[3]]);
    }

    /// `RenderGlobal.renderSky(BufferBuilder, posY, reverseX)`: 13 x 13 quads of 64 at `y`.
    fn plane(&mut self, y: f32, reverse_x: bool, c: [f32; 4]) {
        for k in (-384..=384).step_by(64) {
            for l in (-384..=384).step_by(64) {
                let (mut f, mut f1) = (k as f32, (k + 64) as f32);
                if reverse_x {
                    (f, f1) = (f1, f);
                }
                let (l0, l1) = (l as f32, (l + 64) as f32);
                let p = [Vec3::new(f, y, l0), Vec3::new(f1, y, l0), Vec3::new(f1, y, l1), Vec3::new(f, y, l1)];
                self.quad(p, [[0.0; 2]; 4], K_PLANE, c);
            }
        }
    }

    fn into_mesh(self) -> Mesh {
        Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::RENDER_WORLD)
            .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, self.pos)
            .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, self.uv)
            .with_inserted_attribute(Mesh::ATTRIBUTE_UV_1, self.kind)
            .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, self.color)
            .with_inserted_indices(Indices::U32(self.idx))
    }
}

/// `RenderGlobal.renderSky` for the overworld, in the frame of the view entity's feet (the
/// modelview origin after `orientCamera`'s `translate(0, -eyeHeight, 0)`), drawn in order.
/// Culling is face culling as vanilla has it on: the up-facing planes below the horizon show only
/// while they are under the eye.
pub fn sky_mesh(clock: &Clock, view: &View, stars: &[[[f32; 3]; 4]]) -> Mesh {
    let angle = clock.angle();
    let mut b = SkyBuilder::default();
    let [f, f1, f2] = view.sky;
    // The sky plane: sky color, fogged with setupFog(-1), depth writes off.
    b.plane(16.0, false, [f, f1, f2, 1.0]);

    // The sunrise and sunset fan, blended SRC_ALPHA, ONE_MINUS_SRC_ALPHA, smooth shaded.
    if let Some(a) = celestial::sunrise_colors(angle) {
        let flip = if sin(celestial::celestial_angle_radians(angle)) < 0.0 { 180.0f32 } else { 0.0 };
        let m = Mat3::from_rotation_x(90f32.to_radians())
            * Mat3::from_rotation_z(flip.to_radians())
            * Mat3::from_rotation_z(90f32.to_radians());
        let center = b.vertex(m * Vec3::new(0.0, 100.0, 0.0), [0.0; 2], K_FAN, a);
        let ring: Vec<u32> = (0..=16)
            .map(|j2| {
                let f21 = j2 as f32 * 6.283_185_5 / 16.0;
                let (f12, f13) = (sin(f21), cos(f21));
                let p = m * Vec3::new(f12 * 120.0, f13 * 120.0, -f13 * 40.0 * a[3]);
                b.vertex(p, [0.0; 2], K_FAN, [a[0], a[1], a[2], 0.0])
            })
            .collect();
        for w in ring.windows(2) {
            b.idx.extend_from_slice(&[center, w[0], w[1]]);
        }
    }

    // Sun, moon and stars, blended SRC_ALPHA, ONE in the rotated frame.
    let m = Mat3::from_rotation_y(-90f32.to_radians()) * Mat3::from_rotation_x((angle * 360.0).to_radians());
    let white = [1.0; 4];
    let f17 = 30.0;
    b.quad(
        [
            m * Vec3::new(-f17, 100.0, -f17),
            m * Vec3::new(f17, 100.0, -f17),
            m * Vec3::new(f17, 100.0, f17),
            m * Vec3::new(-f17, 100.0, f17),
        ],
        [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]],
        K_SUN,
        white,
    );
    let f17 = 20.0;
    let k1 = celestial::moon_phase(clock.world_time);
    let (i2, k2) = (k1 % 4, k1 / 4 % 2);
    let f22 = (i2 + 0) as f32 / 4.0;
    let f23 = (k2 + 0) as f32 / 2.0;
    let f24 = (i2 + 1) as f32 / 4.0;
    let f14 = (k2 + 1) as f32 / 2.0;
    b.quad(
        [
            m * Vec3::new(-f17, -100.0, f17),
            m * Vec3::new(f17, -100.0, f17),
            m * Vec3::new(f17, -100.0, -f17),
            m * Vec3::new(-f17, -100.0, -f17),
        ],
        [[f24, f14], [f22, f14], [f22, f23], [f24, f23]],
        K_MOON,
        white,
    );
    let f15 = celestial::star_brightness(angle);
    if f15 > 0.0 {
        for q in stars {
            b.quad(q.map(|p| m * Vec3::from_array(p)), [[0.0; 2]; 4], K_STAR, [f15; 4]);
        }
    }

    // Below the horizon: fog back on, the planes in black and then in the dimmed sky color.
    let below = |y: f32| y - view.eye_height < 0.0;
    // getPositionEyes(partialTicks).y - getHorizon()
    let d3 = (view.feet.y + view.eye_height) as f64 - 63.0;
    if d3 < 0.0 {
        // sky2 (y -16) under translate(0, 12, 0).
        b.plane(-16.0 + 12.0, true, [0.0, 0.0, 0.0, 1.0]);
        // DROPPED: the black box from y -1 down to -(d3 + 65) around the feet. It lies inside
        // the black plane above for every direction it covers, and fogs to the same color.
    }
    let y = -((d3 - 16.0) as f32) - 16.0;
    if below(y) {
        b.plane(y, true, [f * 0.2 + 0.04, f1 * 0.2 + 0.04, f2 * 0.6 + 0.1, 1.0]);
    }
    b.into_mesh()
}

/// Rebuilds the sky mesh at the feet. `EntityRenderer.renderWorldPass` draws the sky only from
/// render distance 4.
pub fn update_sky(
    clock: Res<Clock>,
    settings: Res<Settings>,
    view: Res<View>,
    stars: Res<Stars>,
    mut meshes: ResMut<Assets<Mesh>>,
    sky: Single<(&Mesh3d, &mut Transform, &mut Visibility), With<SkyMesh>>,
) {
    let (mesh, mut transform, mut visibility) = sky.into_inner();
    if settings.render_distance < 4 {
        *visibility = Visibility::Hidden;
        return;
    }
    *visibility = Visibility::Inherited;
    transform.translation = view.feet;
    if let Some(mut m) = meshes.get_mut(&mesh.0) {
        *m = sky_mesh(&clock, &view, &stars.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_clock_follows_the_server() {
        let sync = |world_time, cycle, snap| proto::TimeSync { world_time, cycle, snap };
        let mut c = Clock::new(500);
        c.sync(sync(6000, true, false));
        assert_eq!(c.world_time, 6000, "the first sync jumps");
        // A little behind: two ticks a tick until caught up, then one.
        c.sync(sync(6003, true, false));
        let steps: Vec<i64> = (0..5).map(|_| c.step()).collect();
        assert_eq!(steps, vec![2, 2, 2, 1, 1]);
        // Ahead with the cycle off: back a tick a tick, then stand.
        c.sync(sync(5998, false, false));
        let steps: Vec<i64> = (0..4).map(|_| c.step()).collect();
        assert_eq!(steps, vec![-1, -1, 0, 0]);
        // A /time jump, or far off.
        c.sync(sync(13000, false, true));
        assert_eq!((c.world_time, c.step()), (13000, 0));
        c.sync(sync(13000 + SNAP_TICKS + 1, true, false));
        assert_eq!(c.world_time, 13000 + SNAP_TICKS + 1);
    }

    #[test]
    fn stars_keep_the_ones_inside_the_shell() {
        let s = stars();
        // 1500 tries in the cube, kept inside the unit sphere: about 1500 * pi / 6 = 785.
        assert!(s.len() > 700 && s.len() < 870, "{}", s.len());
        for q in &s {
            for p in q {
                let r = Vec3::from_array(*p).length();
                assert!((99.0..101.0).contains(&r), "{r}");
            }
        }
    }

    #[test]
    fn sun_sets_in_the_west() {
        // Celestial angle 0.25 is sunset: the sun quad's center turns to -x.
        let m = Mat3::from_rotation_y(-90f32.to_radians()) * Mat3::from_rotation_x(90f32.to_radians());
        let c = m * Vec3::new(0.0, 100.0, 0.0);
        assert!((c - Vec3::new(-100.0, 0.0, 0.0)).length() < 1e-3, "{c}");
        let (dir, _) = sun_light(0.25 - 0.01);
        assert!(dir.x < -0.9, "{dir}");
    }

    #[test]
    fn lower_planes_follow_the_eye() {
        let clock = Clock::new(6000);
        let mut view = View::default();
        // Eye at 70: d3 = 7, the dim plane sits at -7, below the eye.
        view.feet = Vec3::new(0.0, 70.0 - 1.62, 0.0);
        let a = sky_mesh(&clock, &view, &[]);
        // Eye at 40: the black plane at -4 joins, the dim plane at 23 is above the eye and culled.
        view.feet = Vec3::new(0.0, 40.0 - 1.62, 0.0);
        let b = sky_mesh(&clock, &view, &[]);
        let quads = |m: &Mesh| m.indices().unwrap().len() / 6;
        assert_eq!(quads(&a), quads(&b));
        let ys = |m: &Mesh| -> Vec<f32> {
            match m.attribute(Mesh::ATTRIBUTE_POSITION) {
                Some(bevy::mesh::VertexAttributeValues::Float32x3(v)) => v.iter().map(|p| p[1]).collect(),
                _ => vec![],
            }
        };
        assert!(ys(&a).iter().any(|&y| (y + 7.0).abs() < 1e-4));
        assert!(ys(&b).iter().any(|&y| (y + 4.0).abs() < 1e-4));
        assert!(!ys(&b).iter().any(|&y| (y - 23.0).abs() < 1e-4));
    }
}
