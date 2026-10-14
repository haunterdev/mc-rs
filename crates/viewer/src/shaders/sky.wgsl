// Sky and clouds. The sky is one mesh drawn in submission order with premultiplied blending,
// which covers the three blend states RenderGlobal.renderSky switches between: opaque fogged
// planes, the alpha-blended sunrise fan, and the additive sun, moon and stars. The vertex's
// uv_b.x picks the case. Clouds (CLOUDS) are the face shade and texel color from the vertex
// times the cloud color, fogged, alpha blended.
#import bevy_pbr::{forward_io::VertexOutput, mesh_view_bindings::view}
#import bevy_render::color_operations::srgb_to_linear

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var env: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(1) var sun: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(2) var sun_sampler: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(3) var moon: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(4) var moon_sampler: sampler;

const P_FOG_COLOR: i32 = 0;
const P_FOG: i32 = 1;
const P_SKY_FOG: i32 = 5;
const P_CLOUD_COLOR: i32 = 6;

fn env_param(slot: i32) -> vec4<f32> {
    return textureLoad(env, vec2<i32>(slot, 16), 0);
}

fn fog(rgb: vec3<f32>, dist: f32, slot: i32) -> vec3<f32> {
    let p = env_param(slot);
    var f: f32;
    if p.w < 0.5 {
        f = (p.y - dist) / max(p.y - p.x, 0.0001);
    } else {
        f = exp(-p.z * dist);
    }
    return mix(env_param(P_FOG_COLOR).rgb, rgb, clamp(f, 0.0, 1.0));
}

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let dist = distance(in.world_position.xyz, view.world_position);
#ifdef CLOUDS
    let rgb = in.color.rgb * env_param(P_CLOUD_COLOR).rgb;
    return vec4(srgb_to_linear(fog(rgb, dist, P_FOG)), in.color.a);
#else
    let sun_texel = textureSample(sun, sun_sampler, in.uv);
    let moon_texel = textureSample(moon, moon_sampler, in.uv);
    let c = in.color;
    switch u32(round(in.uv_b.x)) {
        // Fogged opaque plane (the sky dome above, the dark plane below the horizon).
        case 0u: {
            return vec4(srgb_to_linear(fog(c.rgb, dist, P_SKY_FOG)), 1.0);
        }
        // SRC_ALPHA, ONE_MINUS_SRC_ALPHA.
        case 1u: {
            return vec4(srgb_to_linear(c.rgb) * c.a, c.a);
        }
        // SRC_ALPHA, ONE, textured: the sun.
        case 2u: {
            let t = sun_texel * c;
            return vec4(srgb_to_linear(t.rgb) * t.a, 0.0);
        }
        // SRC_ALPHA, ONE, textured: the moon.
        case 3u: {
            let t = moon_texel * c;
            return vec4(srgb_to_linear(t.rgb) * t.a, 0.0);
        }
        // SRC_ALPHA, ONE, untextured: the stars.
        default: {
            return vec4(srgb_to_linear(c.rgb) * c.a, 0.0);
        }
    }
#endif
}
