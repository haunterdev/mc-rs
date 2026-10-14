// Item fragment shader: the hotbar icons and the held block. RenderItem.renderItemModel's
// fixed-function state: alphaFunc(GL_GREATER, 0.1), the item lights when enabled (ambient 0.4,
// two diffuse lights at 0.6, the vertex color as the material), the texture modulated, and in
// the hand the lightmap after it.
#import bevy_pbr::forward_io::VertexOutput
#import bevy_render::color_operations::srgb_to_linear
#import viewer::chunk_common::{tag_of, sample_block_level0, lightmap, env_param, P_HAND_LIGHT}

struct ItemParams {
    // The two light directions in this camera's world, for the GUI.
    w0: vec4<f32>,
    w1: vec4<f32>,
    // x: lighting on; y: in the hand (lights and lightmap from the environment texture); z: dimmed
    // under the options menu.
    flags: vec4<u32>,
}

@group(#{MATERIAL_BIND_GROUP}) @binding(4) var<uniform> item: ItemParams;

// The hand's light directions in eye space (env.rs), written by hand::update_hand.
const P_HAND_W0: i32 = 7;
const P_HAND_W1: i32 = 8;

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let tex = sample_block_level0(in.uv, tag_of(in.color));
    if tex.a <= 0.1 {
        discard;
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
    var out = srgb_to_linear(rgb);
    if item.flags.z != 0u {
        // Under the options menu's backdrop (settings.rs), as the UI blends it: 0.06 gray at 0.63.
        out = out * 0.37 + srgb_to_linear(vec3(0.06)) * 0.63;
    }
    return vec4(out, tex.a);
}
