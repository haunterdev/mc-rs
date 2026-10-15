//! Block textures read from the client jar: PNG decoding, animation frames, vanilla mipmaps, and
//! the texture array the chunk shader samples.
//!
//! Vanilla stitches every sprite into one atlas and samples it `GL_NEAREST_MIPMAP_LINEAR` with
//! `GL_TEXTURE_MAX_LOD` = the mipmap setting. Here every sprite frame is one layer of a 32x32
//! texture array instead, which has the same texel density per block face: a 16 px sprite's
//! level 0 is a 2x nearest upscale (sampled nearest, it returns the same texel as the 16 px
//! original), and its vanilla level `k` is array level `k + 1`. The 32 px flow sprites map level
//! to level. Animated sprites get one layer per displayed frame, in display order.

use std::collections::HashMap;
use std::io::{self, Cursor};
use std::sync::OnceLock;

use bevy::asset::RenderAssetUsages;
use bevy::image::{Image, ImageAddressMode, ImageFilterMode, ImageSampler, ImageSamplerDescriptor};
use bevy::render::render_resource::{
    Extent3d, TextureDataOrder, TextureDimension, TextureFormat, TextureUsages, TextureViewDescriptor, TextureViewDimension,
};

use crate::jar::Jar;

/// Edge of every array layer, in texels.
pub const LAYER_SIZE: u32 = 32;
/// Mip levels per layer: 32, 16, 8, 4, 2, 1.
pub const ARRAY_LEVELS: u32 = 6;
/// Layers addressable by the 10-bit layer field of the vertex tag (`mesher::FLAG_NOMIP` sits above).
pub const MAX_LAYERS: u32 = 1024;

/// A decoded image as `BufferedImage.getRGB` returns it: ARGB, row-major.
#[derive(Clone)]
pub struct Argb {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u32>,
}

fn bad(msg: String) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, msg)
}

/// Decodes a PNG to ARGB. Palette, grayscale and 16-bit images are expanded to 8-bit RGBA first.
pub fn decode_png(bytes: &[u8]) -> io::Result<Argb> {
    let mut decoder = png::Decoder::new(Cursor::new(bytes));
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16 | png::Transformations::ALPHA);
    let mut reader = decoder.read_info().map_err(|e| bad(format!("png: {e}")))?;
    let mut buf = vec![0; reader.output_buffer_size().ok_or_else(|| bad("png: image too large".into()))?];
    let info = reader.next_frame(&mut buf).map_err(|e| bad(format!("png: {e}")))?;
    let (w, h) = (info.width, info.height);
    let channels = match info.color_type {
        png::ColorType::Grayscale => 1,
        png::ColorType::GrayscaleAlpha => 2,
        png::ColorType::Rgb => 3,
        png::ColorType::Rgba => 4,
        png::ColorType::Indexed => return Err(bad("png: palette survived EXPAND".into())),
    };
    let mut pixels = Vec::with_capacity((w * h) as usize);
    for y in 0..h as usize {
        let row = &buf[y * info.line_size..];
        for x in 0..w as usize {
            let p = &row[x * channels..x * channels + channels];
            let (r, g, b, a) = match channels {
                1 => (p[0], p[0], p[0], 255),
                2 => (p[0], p[0], p[0], p[1]),
                3 => (p[0], p[1], p[2], 255),
                _ => (p[0], p[1], p[2], p[3]),
            };
            pixels.push((a as u32) << 24 | (r as u32) << 16 | (g as u32) << 8 | b as u32);
        }
    }
    Ok(Argb { width: w, height: h, pixels })
}

/// `TextureUtil.COLOR_GAMMAS`: `(float)Math.pow(i / 255F, 2.2)`.
fn color_gammas() -> &'static [f32; 256] {
    static T: OnceLock<[f32; 256]> = OnceLock::new();
    T.get_or_init(|| std::array::from_fn(|i| ((i as f32 / 255.0) as f64).powf(2.2) as f32))
}

/// `TextureUtil.getColorGamma`.
#[inline]
fn gamma(v: u32) -> f32 {
    color_gammas()[(v & 255) as usize]
}

/// `TextureUtil.blendColors`.
fn blend_colors(c: [u32; 4], alpha_aware: bool) -> u32 {
    if alpha_aware {
        let (mut f, mut f1, mut f2, mut f3) = (0f32, 0f32, 0f32, 0f32);
        for p in c {
            if p >> 24 != 0 {
                f += gamma(p >> 24);
                f1 += gamma(p >> 16);
                f2 += gamma(p >> 8);
                f3 += gamma(p);
            }
        }
        let inv = |v: f32| ((v / 4.0) as f64).powf(0.45454545454545453) * 255.0;
        let mut a = inv(f) as i32;
        let (r, g, b) = (inv(f1) as i32, inv(f2) as i32, inv(f3) as i32);
        if a < 96 {
            a = 0;
        }
        ((a as u32) << 24) | ((r as u32) << 16) | ((g as u32) << 8) | b as u32
    } else {
        let comp = |shift: u32| blend_color_component(c[0] >> shift, c[1] >> shift, c[2] >> shift, c[3] >> shift) as u32;
        comp(24) << 24 | comp(16) << 16 | comp(8) << 8 | comp(0)
    }
}

/// `TextureUtil.blendColorComponent`.
fn blend_color_component(a: u32, b: u32, c: u32, d: u32) -> i32 {
    let sum = gamma(a) + gamma(b) + gamma(c) + gamma(d);
    let f4 = ((sum as f64) * 0.25).powf(0.45454545454545453) as f32;
    (f4 as f64 * 255.0) as i32
}

/// `TextureUtil.generateMipmapData(levels, width, data)` for one square frame, where `data` is the
/// frame's `int[levels + 1][]` with only level 0 filled, as `TextureAtlasSprite.loadSpriteFrames`
/// builds it.
///
/// Vanilla decides between the alpha-aware and the plain blend by scanning
/// `data[0][0 .. data.length]`, so only the first `levels + 1` pixels of level 0 are checked for
/// zero alpha. That quirk is kept: the result depends on the mipmap setting.
///
/// `extra` levels past `levels` are generated the same way, to fill the array's smaller mips;
/// the sampler's LOD clamp keeps them from being read.
pub fn generate_mipmap_data(levels: u32, width: u32, level0: &[u32], extra: u32) -> Vec<Vec<u32>> {
    let mut out = vec![level0.to_vec()];
    let total = levels + extra;
    if total == 0 {
        return out;
    }
    let scan = (levels as usize + 1).min(level0.len());
    let flag = level0[..scan].iter().any(|&p| p >> 24 == 0);
    for l1 in 1..=total {
        let prev = &out[l1 as usize - 1];
        let j = (width >> l1) as usize;
        if j == 0 {
            break;
        }
        let len = prev.len() >> 2;
        let k = len / j;
        let l = j << 1;
        let mut next = vec![0u32; len];
        for i1 in 0..j {
            for j1 in 0..k {
                let k1 = 2 * (i1 + j1 * l);
                next[i1 + j1 * j] = blend_colors([prev[k1], prev[k1 + 1], prev[k1 + l], prev[k1 + 1 + l]], flag);
            }
        }
        out.push(next);
    }
    out
}

/// `RenderGlobal.destroyBlockIcons`: `textures/blocks/destroy_stage_0..9`, in the block array for
/// the crack overlay (survival/crack.rs).
pub const DESTROY_SPRITES: [&str; 10] = [
    "blocks/destroy_stage_0",
    "blocks/destroy_stage_1",
    "blocks/destroy_stage_2",
    "blocks/destroy_stage_3",
    "blocks/destroy_stage_4",
    "blocks/destroy_stage_5",
    "blocks/destroy_stage_6",
    "blocks/destroy_stage_7",
    "blocks/destroy_stage_8",
    "blocks/destroy_stage_9",
];

/// One sprite: its frames in display order (level 0 each) and the ticks each is shown for.
pub struct Sprite {
    pub width: u32,
    pub frames: Vec<Vec<u32>>,
    pub frametime: u32,
}

/// Where a sprite sits in the array, as the mesher writes it into vertices.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SpriteInfo {
    pub layer: u32,
    pub frames: u32,
    pub frametime: u32,
}

/// Splits an animation strip by its `.mcmeta`, after `TextureAtlasSprite.loadSpriteFrames` and
/// `AnimationMetadataSectionSerializer`.
pub fn sprite_from(img: &Argb, mcmeta: Option<&serde_json::Value>) -> io::Result<Sprite> {
    let w = img.width;
    let Some(anim) = mcmeta.and_then(|m| m.get("animation")) else {
        if img.width != img.height {
            return Err(bad("broken aspect ratio and not an animation".into()));
        }
        return Ok(Sprite { width: w, frames: vec![img.pixels.clone()], frametime: 1 });
    };
    // Frames are square, `width` on a side (TextureAtlasSprite.loadSprite sets height = width).
    let count = img.height / w;
    let frame = |i: u32| -> io::Result<Vec<u32>> {
        if i >= count {
            return Err(bad(format!("invalid frameindex {i}")));
        }
        let n = (w * w) as usize;
        Ok(img.pixels[i as usize * n..(i as usize + 1) * n].to_vec())
    };
    let default_time = anim.get("frametime").and_then(|v| v.as_u64()).unwrap_or(1) as u32;
    // DROPPED: interpolated animation (TextureAtlasSprite.updateAnimationInterpolated); none of
    // the sprites the generated blocks use sets "interpolate".
    let listed: Vec<(u32, u32)> = match anim.get("frames").and_then(|v| v.as_array()) {
        Some(list) if !list.is_empty() => list
            .iter()
            .map(|f| match f {
                serde_json::Value::Number(n) => (n.as_u64().unwrap_or(0) as u32, default_time),
                obj => (
                    obj.get("index").and_then(|v| v.as_u64()).unwrap_or(0) as u32,
                    obj.get("time").and_then(|v| v.as_u64()).map_or(default_time, |t| t as u32),
                ),
            })
            .collect(),
        _ => (0..count).map(|i| (i, default_time)).collect(),
    };
    // One layer per displayed frame. Frames with their own time are expanded to one layer per
    // tick, so a single frametime covers the whole sequence.
    if listed.iter().all(|&(_, t)| t == listed[0].1) {
        let frames = listed.iter().map(|&(i, _)| frame(i)).collect::<io::Result<_>>()?;
        Ok(Sprite { width: w, frames, frametime: listed[0].1.max(1) })
    } else {
        let mut frames = Vec::new();
        for &(i, t) in &listed {
            let f = frame(i)?;
            for _ in 0..t.max(1) {
                frames.push(f.clone());
            }
        }
        Ok(Sprite { width: w, frames, frametime: 1 })
    }
}

/// Loads `textures/<name>.png` and its `.mcmeta` from the jar.
pub fn load_sprite(jar: &Jar, name: &str) -> io::Result<Sprite> {
    let png = jar.asset(&format!("textures/{name}.png"))?;
    let meta_path = format!("textures/{name}.png.mcmeta");
    let meta = if jar.contains(&format!("assets/minecraft/{meta_path}")) { Some(jar.json(&meta_path)?) } else { None };
    sprite_from(&decode_png(&png)?, meta.as_ref())
}

/// Every block sprite, laid out as array layers.
pub struct BlockTextures {
    pub sprites: Vec<(String, Sprite)>,
    pub index: HashMap<String, SpriteInfo>,
    pub layers: u32,
}

impl BlockTextures {
    pub fn new(sprites: Vec<(String, Sprite)>) -> BlockTextures {
        let mut index = HashMap::new();
        let mut layer = 0;
        for (name, s) in &sprites {
            index.insert(name.clone(), SpriteInfo { layer, frames: s.frames.len() as u32, frametime: s.frametime });
            layer += s.frames.len() as u32;
        }
        BlockTextures { sprites, index, layers: layer }
    }

    pub fn get(&self, name: &str) -> Option<SpriteInfo> {
        self.index.get(name).copied()
    }

    /// The animation table the chunk shader reads: at each sprite's first layer,
    /// `frames | frametime << 16`, four layers to an entry.
    pub fn anim_table(&self) -> [[u32; 4]; 256] {
        assert!(self.layers <= MAX_LAYERS, "{} texture layers, the vertex tag holds {MAX_LAYERS}", self.layers);
        let mut t = [[0u32; 4]; 256];
        for info in self.index.values() {
            let l = info.layer as usize;
            t[l / 4][l % 4] = info.frames | info.frametime << 16;
        }
        t
    }

    /// RGBA8 bytes for every layer and level, layer-major (each layer's mips before the next
    /// layer), as `TextureDataOrder::LayerMajor` expects.
    pub fn array_data(&self, mip_setting: u32) -> Vec<u8> {
        let mut out = Vec::new();
        for (_, s) in &self.sprites {
            for f in &s.frames {
                let (scale, extra) = if s.width == LAYER_SIZE { (1, ARRAY_LEVELS - 1) } else { (LAYER_SIZE / s.width, ARRAY_LEVELS - 2) };
                let extra = extra.saturating_sub(mip_setting);
                let mips = generate_mipmap_data(mip_setting, s.width, f, extra);
                if scale == 2 {
                    // Level 0: the 16 px frame upscaled 2x, nearest.
                    let w = s.width as usize;
                    for y in 0..LAYER_SIZE as usize {
                        for x in 0..LAYER_SIZE as usize {
                            push_rgba(&mut out, mips[0][(y / 2) * w + x / 2]);
                        }
                    }
                }
                for level in mips.iter().take((ARRAY_LEVELS - (scale - 1)) as usize) {
                    for &p in level {
                        push_rgba(&mut out, p);
                    }
                }
            }
        }
        out
    }

    /// The array texture. Sampling matches the atlas: nearest within a level, linear between
    /// levels, LOD clamped to the mipmap setting.
    pub fn image(&self, mip_setting: u32) -> Image {
        let mut image = Image::new_uninit(
            Extent3d { width: LAYER_SIZE, height: LAYER_SIZE, depth_or_array_layers: self.layers.max(1) },
            TextureDimension::D2,
            // Raw texel values, as the atlas holds them: the shader works in vanilla's gamma
            // space and mip filtering stays unconverted, like GL on a non-sRGB texture.
            TextureFormat::Rgba8Unorm,
            RenderAssetUsages::RENDER_WORLD,
        );
        image.data = Some(self.array_data(mip_setting));
        image.data_order = TextureDataOrder::LayerMajor;
        image.texture_descriptor.mip_level_count = ARRAY_LEVELS;
        image.texture_descriptor.usage = TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST;
        image.texture_view_descriptor =
            Some(TextureViewDescriptor { dimension: Some(TextureViewDimension::D2Array), ..Default::default() });
        image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
            label: Some("block array".into()),
            address_mode_u: ImageAddressMode::ClampToEdge,
            address_mode_v: ImageAddressMode::ClampToEdge,
            mag_filter: ImageFilterMode::Nearest,
            min_filter: ImageFilterMode::Nearest,
            mipmap_filter: ImageFilterMode::Linear,
            // Array level `k + 1` is vanilla level `k` for the 16 px sprites. With mipmaps off,
            // level 0 alone gives vanilla's GL_NEAREST.
            lod_max_clamp: if mip_setting == 0 { 0.0 } else { mip_setting as f32 + 1.0 },
            ..Default::default()
        });
        image
    }
}

/// A plain 2D texture (sun, moon, player skin, GUI icons), sampled `GL_NEAREST` like
/// `TextureManager` binds them. `srgb` for the UI, which blends in linear space; the world
/// shaders take raw texel values like the atlas.
pub fn flat_image(img: &Argb, srgb: bool) -> Image {
    let mut data = Vec::with_capacity(img.pixels.len() * 4);
    for &p in &img.pixels {
        push_rgba(&mut data, p);
    }
    let mut image = Image::new(
        Extent3d { width: img.width, height: img.height, depth_or_array_layers: 1 },
        TextureDimension::D2,
        data,
        if srgb { TextureFormat::Rgba8UnormSrgb } else { TextureFormat::Rgba8Unorm },
        RenderAssetUsages::RENDER_WORLD,
    );
    image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        label: Some("flat".into()),
        address_mode_u: ImageAddressMode::Repeat,
        address_mode_v: ImageAddressMode::Repeat,
        mag_filter: ImageFilterMode::Nearest,
        min_filter: ImageFilterMode::Nearest,
        ..Default::default()
    });
    image
}

fn push_rgba(out: &mut Vec<u8>, argb: u32) {
    out.extend_from_slice(&[(argb >> 16) as u8, (argb >> 8) as u8, argb as u8, (argb >> 24) as u8]);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mipmaps_average_in_gamma_space() {
        // 2x2 opaque: black, white, black, white. Plain blend: (0 + 1 + 0 + 1) / 4 = 0.5 linear,
        // 0.5 ^ (1 / 2.2) * 255 = 186.
        let w = 0xFFFF_FFFFu32;
        let b = 0xFF00_0000u32;
        let m = generate_mipmap_data(1, 2, &[b, w, b, w], 0);
        assert_eq!(m.len(), 2);
        assert_eq!(m[1], vec![0xFFBA_BABA]);
    }

    #[test]
    fn transparent_texels_drop_out_of_the_blend() {
        // First pixel transparent, so the alpha-aware blend runs: only the opaque white texels
        // count, and each sum is still divided by four.
        let m = generate_mipmap_data(1, 2, &[0x0000_0000, 0xFFFF_FFFF, 0xFFFF_FFFF, 0xFFFF_FFFF], 0);
        let expect = ((0.75f32 as f64).powf(0.45454545454545453) * 255.0) as u32;
        assert_eq!(m[1][0], expect << 24 | expect << 16 | expect << 8 | expect);
    }

    #[test]
    fn alpha_scan_only_reads_the_first_levels_plus_one_pixels() {
        // A transparent white texel at index 3 is outside the scan at mipmap level 1 (two
        // pixels), so its color still counts there; at level 3 the scan reaches it.
        let px = [0xFF00_0000, 0xFFFF_FFFF, 0xFF00_0000, 0x00FF_FFFF];
        let plain = generate_mipmap_data(1, 2, &px, 0);
        let aware = generate_mipmap_data(3, 2, &px, 0);
        assert_ne!(plain[1], aware[1]);
        assert_eq!(plain[1][0] >> 24, blend_color_component(0xFF, 0xFF, 0xFF, 0) as u32);
    }

    #[test]
    fn animation_frames_follow_the_frames_list() {
        let img = Argb { width: 2, height: 6, pixels: (0..12).collect() };
        let meta = serde_json::json!({"animation": {"frametime": 2, "frames": [2, 0, 1, 0]}});
        let s = sprite_from(&img, Some(&meta)).unwrap();
        assert_eq!(s.frametime, 2);
        assert_eq!(s.frames, vec![vec![8, 9, 10, 11], vec![0, 1, 2, 3], vec![4, 5, 6, 7], vec![0, 1, 2, 3]]);
        let plain = sprite_from(&img, Some(&serde_json::json!({"animation": {}}))).unwrap();
        assert_eq!((plain.frames.len(), plain.frametime), (3, 1));
    }

    #[test]
    fn array_layers_hold_every_level() {
        let red = Sprite { width: 16, frames: vec![vec![0xFFFF_0000; 256]], frametime: 1 };
        let flow = Sprite { width: 32, frames: vec![vec![0xFF00_00FF; 1024]; 2], frametime: 1 };
        let t = BlockTextures::new(vec![("a".into(), red), ("b".into(), flow)]);
        assert_eq!(t.layers, 3);
        assert_eq!(t.get("b"), Some(SpriteInfo { layer: 1, frames: 2, frametime: 1 }));
        let anim = t.anim_table();
        assert_eq!(anim[0], [1 | 1 << 16, 2 | 1 << 16, 0, 0]);
        let per_layer: usize = (0..ARRAY_LEVELS).map(|l| ((LAYER_SIZE >> l) * (LAYER_SIZE >> l) * 4) as usize).sum();
        for setting in 0..=4 {
            let data = t.array_data(setting);
            assert_eq!(data.len(), per_layer * 3, "mip setting {setting}");
            assert_eq!(&data[..4], &[255, 0, 0, 255]);
            assert_eq!(&data[per_layer..per_layer + 4], &[0, 0, 255, 255]);
        }
    }

    #[test]
    fn decodes_jar_textures() {
        let Ok(jar) = Jar::find() else {
            eprintln!("skipped: no client jar");
            return;
        };
        let water = load_sprite(&jar, "blocks/water_still").unwrap();
        assert_eq!((water.width, water.frames.len(), water.frametime), (16, 32, 2));
        let lava = load_sprite(&jar, "blocks/lava_still").unwrap();
        assert_eq!((lava.frames.len(), lava.frametime), (38, 2));
        let flow = load_sprite(&jar, "blocks/water_flow").unwrap();
        assert_eq!((flow.width, flow.frames.len(), flow.frametime), (32, 32, 1));
        let stone = load_sprite(&jar, "blocks/stone").unwrap();
        assert!(stone.frames[0].iter().all(|p| p >> 24 == 0xFF));
    }
}
