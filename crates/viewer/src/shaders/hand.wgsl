// The first-person arm: the skin texel, alpha tested at 0.1 and blended (RenderPlayer.renderRightArm
// enables blending), lit by the two fixed-function item lights and the lightmap at the eye.
// The hand camera sits at the origin unrotated, so world space here is vanilla's eye space.
// No fog: renderWorldPass disables it before renderHand.
#import bevy_pbr::forward_io::VertexOutput
#import bevy_render::color_operations::srgb_to_linear

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var env: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(1) var skin: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(2) var skin_sampler: sampler;

const P_HAND_LIGHT: i32 = 3;
const P_HAND_W0: i32 = 7;
const P_HAND_W1: i32 = 8;

fn env_param(slot: i32) -> vec4<f32> {
    return textureLoad(env, vec2<i32>(slot, 16), 0);
}

// The lightmap sampled GL_LINEAR at (block, sky) in 0..240, as chunk_common.wgsl does.
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

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let tex = textureSample(skin, skin_sampler, in.uv);
    // alphaFunc(GL_GREATER, 0.1)
    if tex.a <= 0.1 {
        discard;
    }
    // Culling is off; GL two-sided lighting is off too, so a back face keeps the front normal.
    let n = normalize(in.world_normal);
    // RenderHelper.enableStandardItemLighting: diffuse 0.6 per light, model ambient 0.4.
    let w0 = env_param(P_HAND_W0).xyz;
    let w1 = env_param(P_HAND_W1).xyz;
    let light = min(0.4 + 0.6 * max(dot(n, w0), 0.0) + 0.6 * max(dot(n, w1), 0.0), 1.0);
    let lm = lightmap(env_param(P_HAND_LIGHT).xy);
    return vec4(srgb_to_linear(tex.rgb * light * lm), tex.a);
}
