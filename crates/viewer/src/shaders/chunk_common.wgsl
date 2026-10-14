// Shared by the chunk shaders: the material bindings, texture animation, block sampling, the
// lightmap lookup and fog. Only material bindings here, so the prepass can import it too.
#define_import_path viewer::chunk_common

struct ChunkParams {
    // Per array layer: frames | frametime << 16, four layers to a vec4.
    anim: array<vec4<u32>, 256>,
}

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> params: ChunkParams;
@group(#{MATERIAL_BIND_GROUP}) @binding(1) var blocks: texture_2d_array<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(2) var blocks_sampler: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(3) var env: texture_2d<f32>;

const LAYER_MASK: u32 = 1023u;
const FLAG_NOMIP: u32 = 1024u;
const FLAG_EMISSIVE: u32 = 2048u;

// Parameter texels in row 16 of the environment texture (env.rs).
const P_FOG_COLOR: i32 = 0;
const P_FOG: i32 = 1;
const P_SHADING: i32 = 2;
const P_HAND_LIGHT: i32 = 3;
const P_TIME: i32 = 4;

fn env_param(slot: i32) -> vec4<f32> {
    return textureLoad(env, vec2<i32>(slot, 16), 0);
}

// The array layer and flags the mesher wrote into the vertex color's alpha.
fn tag_of(color: vec4<f32>) -> u32 {
    return u32(round(color.a));
}

// TextureAtlasSprite.updateAnimation: the frame shown at the current client tick.
fn layer_of(tag: u32) -> u32 {
    let base = tag & LAYER_MASK;
    let a = params.anim[base / 4u][base % 4u];
    let frames = max(a & 65535u, 1u);
    let frametime = max(a >> 16u, 1u);
    let tick = u32(env_param(P_TIME).x);
    return base + (tick / frametime) % frames;
}

// The block texture at sprite-relative uv. Unmipped sprites (BlockRenderLayer.CUTOUT) pass zero
// gradients, which selects level 0 like vanilla's GL_NEAREST for that layer.
fn sample_block(uv: vec2<f32>, tag: u32) -> vec4<f32> {
    let g = select(1.0, 0.0, (tag & FLAG_NOMIP) != 0u);
    return textureSampleGrad(blocks, blocks_sampler, uv, layer_of(tag), dpdx(uv) * g, dpdy(uv) * g);
}

// The block texture's top level, for items: RenderItem.renderItemModel draws with
// setBlurMipmap(false, false), GL_NEAREST with no mipmaps.
fn sample_block_level0(uv: vec2<f32>, tag: u32) -> vec4<f32> {
    return textureSampleLevel(blocks, blocks_sampler, uv, layer_of(tag), 0.0);
}

// The 16x16 lightmap texture sampled GL_LINEAR at lightmap coordinates (block, sky) in 0..240.
fn lightmap(c: vec2<f32>) -> vec3<f32> {
    let t = clamp(c / 16.0, vec2(0.0), vec2(15.0));
    let i = vec2<i32>(floor(t));
    let f = t - floor(t);
    let j = min(i + vec2(1), vec2(15));
    let a = textureLoad(env, i, 0).rgb;
    let b = textureLoad(env, vec2(j.x, i.y), 0).rgb;
    let c0 = textureLoad(env, vec2(i.x, j.y), 0).rgb;
    let d = textureLoad(env, j, 0).rgb;
    return mix(mix(a, b, f.x), mix(c0, d, f.x), f.y);
}

// Fixed-function fog over the radial eye distance (GL_NV_fog_distance, EYE_RADIAL).
fn apply_fog(rgb: vec3<f32>, dist: f32) -> vec3<f32> {
    let fog = env_param(P_FOG);
    var f: f32;
    if fog.w < 0.5 {
        f = (fog.y - dist) / max(fog.y - fog.x, 0.0001);
    } else {
        f = exp(-fog.z * dist);
    }
    return mix(env_param(P_FOG_COLOR).rgb, rgb, clamp(f, 0.0, 1.0));
}
