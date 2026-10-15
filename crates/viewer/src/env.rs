//! The environment texture: the per-frame values every world shader reads (lightmap, fog, shading
//! mode, the hand's light). The CPU copy is extracted each frame it changes and written into the
//! same GPU texture in place, so the materials binding it never change and no mesh gets
//! re-specialized when the time of day moves.
//!
//! Layout, `Rgba32Float`, 16 x 17 texels, read with `textureLoad`:
//! - rows 0..16: the lightmap, texel `(block, sky)`, RGB in 0..=1 as vanilla's 8-bit texture holds it
//! - row 16: the parameter texels named by the `P_*` constants

use bevy::asset::RenderAssetUsages;
use bevy::prelude::*;
use bevy::render::extract_resource::{ExtractResource, ExtractResourcePlugin};
use bevy::render::render_asset::RenderAssets;
use bevy::render::render_resource::{Extent3d, TexelCopyBufferLayout, TextureDimension, TextureFormat, TextureUsages};
use bevy::render::renderer::RenderQueue;
use bevy::render::texture::GpuImage;
use bevy::render::{Render, RenderApp, RenderSystems};

pub const WIDTH: u32 = 16;
pub const HEIGHT: u32 = 17;

/// Fog color, RGB in vanilla's gamma space.
pub const P_FOG_COLOR: usize = 0;
/// Fog: start, end, density, mode (0 linear, 1 exp).
pub const P_FOG: usize = 1;
/// Shading: mode (0 vanilla, 1 shaded), sun strength, ambient strength, shadows (0 or 1).
pub const P_SHADING: usize = 2;
/// The hand's lightmap coordinates: block, sky (0..=240).
pub const P_HAND_LIGHT: usize = 3;
/// Animation clock: client tick, partial tick. The prepass cannot read the view's `globals`.
pub const P_TIME: usize = 4;
/// Sky fog (`setupFog(-1)`): start, end, density, mode, as `P_FOG`.
pub const P_SKY_FOG: usize = 5;
/// `World.getCloudColour`, RGB.
pub const P_CLOUD_COLOR: usize = 6;
/// `RenderHelper.enableStandardItemLighting`'s two light directions in the hand camera's space,
/// as `ItemRenderer.rotateArroundXAndY` leaves them.
pub const P_HAND_W0: usize = 7;
pub const P_HAND_W1: usize = 8;

#[derive(Resource, Clone, ExtractResource)]
pub struct Env {
    pub image: Handle<Image>,
    pub data: Vec<[f32; 4]>,
}

impl Env {
    fn texel(&mut self, x: usize, y: usize) -> &mut [f32; 4] {
        &mut self.data[y * WIDTH as usize + x]
    }

    pub fn set_lightmap(&mut self, colors: &[u32; 256]) {
        for (i, c) in colors.iter().enumerate() {
            let (sky, block) = (i / 16, i % 16);
            let ch = |shift: u32| ((c >> shift) & 255) as f32 / 255.0;
            *self.texel(block, sky) = [ch(16), ch(8), ch(0), 1.0];
        }
    }

    /// The lightmap's `GL_LINEAR` lookup at coordinates `(block, sky)` in 0..=240, RGB in 0..=1:
    /// what the fixed-function path gives a draw whose entity set `setLightmapTextureCoords`.
    pub fn light_rgb(&self, block: f32, sky: f32) -> [f32; 3] {
        let (u, v) = ((block / 16.0).clamp(0.0, 15.0), (sky / 16.0).clamp(0.0, 15.0));
        let (u0, v0) = (u.floor(), v.floor());
        let (fu, fv) = (u - u0, v - v0);
        let (u0, v0) = (u0 as usize, v0 as usize);
        let (u1, v1) = ((u0 + 1).min(15), (v0 + 1).min(15));
        let t = |x: usize, y: usize| self.data[y * WIDTH as usize + x];
        std::array::from_fn(|ch| {
            let top = t(u0, v0)[ch] * (1.0 - fu) + t(u1, v0)[ch] * fu;
            let bottom = t(u0, v1)[ch] * (1.0 - fu) + t(u1, v1)[ch] * fu;
            top * (1.0 - fv) + bottom * fv
        })
    }

    pub fn set(&mut self, slot: usize, v: [f32; 4]) {
        *self.texel(slot, 16) = v;
    }
}

pub struct EnvPlugin;

impl Plugin for EnvPlugin {
    fn build(&self, app: &mut App) {
        let mut image = Image::new(
            Extent3d { width: WIDTH, height: HEIGHT, depth_or_array_layers: 1 },
            TextureDimension::D2,
            vec![0; (WIDTH * HEIGHT * 16) as usize],
            TextureFormat::Rgba32Float,
            RenderAssetUsages::RENDER_WORLD,
        );
        image.texture_descriptor.usage = TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST;
        let handle = app.world_mut().resource_mut::<Assets<Image>>().add(image);
        app.insert_resource(Env { image: handle, data: vec![[0.0; 4]; (WIDTH * HEIGHT) as usize] })
            .add_plugins(ExtractResourcePlugin::<Env>::default());
        if let Some(render) = app.get_sub_app_mut(RenderApp) {
            render.add_systems(Render, upload.in_set(RenderSystems::PrepareResources));
        }
    }
}

fn upload(env: Option<Res<Env>>, images: Res<RenderAssets<GpuImage>>, queue: Res<RenderQueue>) {
    let Some(env) = env else { return };
    let Some(gpu) = images.get(&env.image) else { return };
    let bytes: Vec<u8> = env.data.iter().flat_map(|t| t.iter().flat_map(|f| f.to_le_bytes())).collect();
    queue.write_texture(
        gpu.texture.as_image_copy(),
        &bytes,
        TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(WIDTH * 16), rows_per_image: Some(HEIGHT) },
        Extent3d { width: WIDTH, height: HEIGHT, depth_or_array_layers: 1 },
    );
}
