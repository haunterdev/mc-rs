// Distant terrain (lod.rs). The vertex color holds the cell's color with the face shade baked in,
// in the block textures' gamma space; it is lit at full sky light like the chunk shader's vanilla
// path, or by the sun and ambient in the shaded mode, then fogged. The quads over a chunk whose
// full mesh is on screen never reach here: lod_vertex.wgsl collapses them, so nothing discards
// and the depth test runs early.
#import bevy_pbr::{
    forward_io::VertexOutput,
    mesh_view_bindings::{view, lights},
}
#import bevy_render::color_operations::{srgb_to_linear, linear_to_srgb}
#import viewer::chunk_common::{lightmap, apply_fog, env_param, P_SHADING}

fn face_shade(n: vec3<f32>) -> f32 {
    let a = abs(n);
    if a.y >= a.x && a.y >= a.z {
        return select(0.5, 1.0, n.y > 0.0);
    }
    return select(0.6, 0.8, a.z >= a.x);
}

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let p = in.world_position.xyz;
    var n = normalize(cross(dpdx(p), dpdy(p)));
    if dot(n, view.world_position - p) < 0.0 {
        n = -n;
    }
    let lm = lightmap(vec2(0.0, 240.0));
    var rgb = in.color.rgb * lm;
    let shading = env_param(P_SHADING);
    if shading.x > 0.5 {
        let albedo = srgb_to_linear(min(in.color.rgb / face_shade(n), vec3(1.0)));
        var direct = 0.0;
        if lights.n_directional_lights > 0u {
            direct = max(dot(n, lights.directional_lights[0].direction_to_light), 0.0) * shading.y;
        }
        rgb = linear_to_srgb(albedo * (vec3(direct) + srgb_to_linear(lm) * shading.z));
    }
    rgb = apply_fog(rgb, distance(p, view.world_position));
    return vec4(srgb_to_linear(rgb), 1.0);
}
