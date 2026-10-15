// Chunk fragment shader. Vanilla mode is the fixed-function terrain path in gamma space:
// texture * vertex color * lightmap, then fog. Shaded mode swaps the baked face shade for a
// shadowed sun and keeps the lightmap as ambient.
#import bevy_pbr::{
    forward_io::VertexOutput,
    mesh_view_bindings::{view, lights},
    mesh_view_types::DIRECTIONAL_LIGHT_FLAGS_SHADOWS_ENABLED_BIT,
    shadows::fetch_directional_shadow,
}
#import bevy_render::color_operations::{srgb_to_linear, linear_to_srgb}
#import viewer::chunk_common::{tag_of, sample_block, lightmap, apply_fog, env_param, P_SHADING, FLAG_EMISSIVE}

// FaceBakery.getFaceBrightness for the face the normal points along.
fn face_shade(n: vec3<f32>) -> f32 {
    let a = abs(n);
    if a.y >= a.x && a.y >= a.z {
        return select(0.5, 1.0, n.y > 0.0);
    }
    return select(0.6, 0.8, a.z >= a.x);
}

fn shaded(in: VertexOutput, tex: vec3<f32>, lm: vec3<f32>, tag: u32, shading: vec4<f32>) -> vec3<f32> {
    if (tag & FLAG_EMISSIVE) != 0u {
        return tex * in.color.rgb;
    }
    let n = normalize(in.world_normal);
    // The vertex color holds face shade * ambient occlusion * biome tint.
    let albedo = srgb_to_linear(tex * min(in.color.rgb / face_shade(n), vec3(1.0)));
    let sky = clamp(in.uv_b.y / 240.0, 0.0, 1.0);
    var direct = 0.0;
    if lights.n_directional_lights > 0u {
        let light = lights.directional_lights[0];
        let ndl = max(dot(n, light.direction_to_light), 0.0);
        var shadow = 1.0;
        if ndl > 0.0 && shading.w > 0.5 && (light.flags & DIRECTIONAL_LIGHT_FLAGS_SHADOWS_ENABLED_BIT) != 0u {
            let view_z = dot(vec4(
                view.view_from_world[0].z,
                view.view_from_world[1].z,
                view.view_from_world[2].z,
                view.view_from_world[3].z,
            ), in.world_position);
            shadow = fetch_directional_shadow(0u, in.world_position, n, view_z, in.position.xy);
        }
        direct = ndl * shadow * sky * sky * shading.y;
    }
    let ambient = srgb_to_linear(lm) * shading.z;
    return linear_to_srgb(albedo * (vec3(direct) + ambient));
}

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let tag = tag_of(in.color);
    let tex = sample_block(in.uv, tag);
#ifdef MAY_DISCARD
    // alphaFunc(GL_GREATER, 0.5) for the cutout layers.
    if tex.a <= 0.5 {
        discard;
    }
#endif
#ifdef ALPHA_TEST_TENTH
    // alphaFunc(GL_GREATER, 0.1) for the translucent layer.
    if tex.a <= 0.1 {
        discard;
    }
#endif
#ifdef CRACK
    // color(1, 1, 1, 0.5) with blendFunc(DST_COLOR, SRC_COLOR): the texel as is, 0.5 is neutral.
    return vec4(tex.rgb, 1.0);
#endif
    let lm = lightmap(in.uv_b);
    var rgb = tex.rgb * in.color.rgb * lm;
    let shading = env_param(P_SHADING);
    if shading.x > 0.5 {
        rgb = shaded(in, tex.rgb, lm, tag, shading);
    }
    rgb = apply_fog(rgb, distance(in.world_position.xyz, view.world_position));
#ifdef ALPHA_TEST_TENTH
    // The translucent layer blends by the texel alpha.
    let alpha = tex.a;
#else
    // Vanilla's window shows the framebuffer opaque. Here the main camera's output is
    // composited over the sky camera's, so a mipped cutout texel's alpha (0.5 to 1 at grass
    // overlay edges) would mix in the void plane as a dark blue tint.
    let alpha = 1.0;
#endif
    return vec4(srgb_to_linear(rgb), alpha);
}
