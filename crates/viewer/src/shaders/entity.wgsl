// Other players (players.rs): the skin texel alpha tested at 0.1, lit the way vanilla lights an
// entity, then fogged. RenderHelper.enableStandardItemLighting sets two fixed lights in world
// space (toward (0.2, 1, -0.7) and (-0.2, 1, 0.7)) with diffuse 0.6 each over a model ambient of
// 0.4, and the lightmap at the entity's eyes (getBrightnessForRender) scales the result.
#import bevy_pbr::{forward_io::VertexOutput, mesh_view_bindings::view}
#import bevy_render::color_operations::srgb_to_linear

struct EntityParams {
    // Lightmap coordinates (block, sky) in 0..240.
    light: vec4<f32>,
}

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var env: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(1) var skin: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(2) var skin_sampler: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(3) var<uniform> params: EntityParams;

const P_FOG_COLOR: i32 = 0;
const P_FOG: i32 = 1;

fn env_param(slot: i32) -> vec4<f32> {
    return textureLoad(env, vec2<i32>(slot, 16), 0);
}

// As chunk_common.wgsl, which this material cannot import (its bindings differ).
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

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let tex = textureSample(skin, skin_sampler, in.uv);
    // alphaFunc(GL_GREATER, 0.1)
    if tex.a <= 0.1 {
        discard;
    }
    let n = normalize(in.world_normal);
    let l0 = normalize(vec3(0.2, 1.0, -0.7));
    let l1 = normalize(vec3(-0.2, 1.0, 0.7));
    let light = min(0.4 + 0.6 * max(dot(n, l0), 0.0) + 0.6 * max(dot(n, l1), 0.0), 1.0);
    var rgb = tex.rgb * light * lightmap(params.light.xy);
    rgb = apply_fog(rgb, distance(in.world_position.xyz, view.world_position));
    return vec4(srgb_to_linear(rgb), 1.0);
}
