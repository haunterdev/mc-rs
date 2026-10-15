// Item fragment shader: the hotbar icons and the held block. RenderItem.renderItemModel's
// fixed-function state: alphaFunc(GL_GREATER, 0.1), the item lights when enabled (ambient 0.4,
// two diffuse lights at 0.6, the vertex color as the material), the texture modulated, and in
// the hand the lightmap after it.
#import bevy_pbr::{forward_io::VertexOutput, mesh_view_bindings::view}
#import bevy_render::color_operations::srgb_to_linear
#import viewer::chunk_common::{tag_of, sample_block_level0, lightmap, apply_fog, env_param, P_HAND_LIGHT}

struct ItemParams {
    // The two light directions in this camera's world, for the GUI and the world.
    w0: vec4<f32>,
    w1: vec4<f32>,
    // x: lighting on; y: in the hand (lights and lightmap from the environment texture); z: dimmed
    // under the options menu; w: in another player's hand in the world (LayerHeldItem), lit as
    // entities are, the lightmap at (w0.w, w1.w), then fogged.
    flags: vec4<u32>,
    // x: the glint pass over the item (RenderItem.renderEffect); y, z: its two layers' scroll.
    glint: vec4<f32>,
}

@group(#{MATERIAL_BIND_GROUP}) @binding(4) var<uniform> item: ItemParams;
@group(#{MATERIAL_BIND_GROUP}) @binding(5) var glint_texture: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(6) var glint_sampler: sampler;

// One layer of the glint: the texture matrix scale(8), translate(scroll, 0), rotate(angle).
fn glint_layer(uv: vec2<f32>, degrees: f32, scroll: f32) -> vec3<f32> {
    let a = radians(degrees);
    let r = vec2(cos(a) * uv.x - sin(a) * uv.y, sin(a) * uv.x + cos(a) * uv.y);
    return textureSampleLevel(glint_texture, glint_sampler, (r + vec2(scroll, 0.0)) * 8.0, 0.0).rgb;
}

// The hand's light directions in eye space (env.rs), written by hand::update_hand.
const P_HAND_W0: i32 = 7;
const P_HAND_W1: i32 = 8;

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let tex = sample_block_level0(in.uv, tag_of(in.color));
    if tex.a <= 0.1 {
        discard;
    }
    if item.glint.x != 0.0 {
        // Both layers tinted 0xFF8040CC, blended as src * src + dst: the sum of the squares comes
        // from the square root of it.
        let tint = vec3(128.0, 64.0, 204.0) / 255.0;
        let a = glint_layer(in.uv, -50.0, item.glint.y) * tint;
        let b = glint_layer(in.uv, 10.0, item.glint.z) * tint;
        let v = srgb_to_linear(a * a + b * b);
        return vec4(sqrt(v), 1.0);
    }
    let hand = item.flags.y != 0u;
    var color = in.color.rgb;
    if item.flags.x != 0u {
        var w0 = item.w0.xyz;
        var w1 = item.w1.xyz;
        if hand {
            w0 = env_param(P_HAND_W0).xyz;
            w1 = env_param(P_HAND_W1).xyz;
        }
        let n = normalize(in.world_normal);
        let light = 0.4 + 0.6 * max(dot(n, w0), 0.0) + 0.6 * max(dot(n, w1), 0.0);
        color = min(color * light, vec3(1.0));
    }
    var rgb = tex.rgb * color;
    if hand {
        rgb *= lightmap(env_param(P_HAND_LIGHT).xy);
    }
    if item.flags.w != 0u {
        rgb *= lightmap(vec2(item.w0.w, item.w1.w));
        rgb = apply_fog(rgb, distance(in.world_position.xyz, view.world_position));
    }
    var out = srgb_to_linear(rgb);
    if item.flags.z != 0u {
        // Under the options menu's backdrop (settings.rs), as the UI blends it: 0.06 gray at 0.63.
        out = out * 0.37 + srgb_to_linear(vec3(0.06)) * 0.63;
    }
    return vec4(out, tex.a);
}
