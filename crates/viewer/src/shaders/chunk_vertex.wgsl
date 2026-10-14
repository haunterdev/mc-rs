// Chunk vertex shader for the main pass, the prepass and the shadow pass: unpacks the 20-byte
// vertex vertex.rs writes into the attributes the fragment shaders read.
#import bevy_pbr::{mesh_functions, view_transformations::position_world_to_clip}
#ifdef PREPASS_PIPELINE
#import bevy_pbr::prepass_io::VertexOutput
#else
#import bevy_pbr::forward_io::VertexOutput
#endif

struct Vertex {
    @builtin(instance_index) instance_index: u32,
    @location(0) packed: vec4<u32>,
    @location(1) normal: u32,
}

@vertex
fn vertex(v: Vertex) -> VertexOutput {
    var out: VertexOutput;
    let p = v.packed;
    let local = vec4(
        f32(p.x & 0xFFFFu) / 2048.0 - 16.0,
        f32(p.y >> 12u) / 2048.0 - 8.0,
        f32(p.x >> 16u) / 2048.0 - 16.0,
        1.0,
    );
    let world_from_local = mesh_functions::get_world_from_local(v.instance_index);
    out.world_position = mesh_functions::mesh_position_local_to_world(world_from_local, local);
    out.position = position_world_to_clip(out.world_position.xyz);
#ifdef UNCLIPPED_DEPTH_ORTHO_EMULATION
    out.unclipped_depth = out.position.z;
    out.position.z = min(out.position.z, 1.0);
#endif
    let n = vec3(
        f32(extractBits(bitcast<i32>(v.normal), 8u, 8u)),
        f32(extractBits(bitcast<i32>(v.normal), 16u, 8u)),
        f32(extractBits(bitcast<i32>(v.normal), 24u, 8u)),
    ) / 127.0;
#ifdef PREPASS_PIPELINE
#ifdef NORMAL_PREPASS_OR_DEFERRED_PREPASS
    out.world_normal = mesh_functions::mesh_normal_local_to_world(n, v.instance_index);
#endif
#ifdef MOTION_VECTOR_PREPASS
    let prev = mesh_functions::get_previous_world_from_local(v.instance_index);
    out.previous_world_position = mesh_functions::mesh_position_local_to_world(prev, local);
#endif
#else
    out.world_normal = mesh_functions::mesh_normal_local_to_world(n, v.instance_index);
#endif
    out.uv = vec2(f32(p.z & 0xFFFFu), f32(p.z >> 16u)) / 32768.0 - 0.5;
    out.uv_b = vec2(f32(p.w >> 24u), f32(v.normal & 0xFFu));
    out.color = vec4(
        vec3(f32(p.w & 0xFFu), f32((p.w >> 8u) & 0xFFu), f32((p.w >> 16u) & 0xFFu)) / 255.0,
        f32(p.y & 0xFFFu),
    );
#ifdef VERTEX_OUTPUT_INSTANCE_INDEX
    out.instance_index = v.instance_index;
#endif
    return out;
}
