// Distant terrain vertex shader (lod.rs): the 20-byte vertex (position, then the packed color and
// chunk lod_mesh.rs writes). A quad over a chunk whose full mesh is on screen collapses to one
// point, read from the shown-chunk mask, so the real terrain replaces it with no overdraw.
#import bevy_pbr::{mesh_functions, view_transformations::position_world_to_clip, forward_io::VertexOutput}

@group(#{MATERIAL_BIND_GROUP}) @binding(4) var mask: texture_2d<f32>;

struct Vertex {
    @builtin(instance_index) instance_index: u32,
    @location(0) position: vec3<f32>,
    @location(1) packed: vec2<u32>,
}

@vertex
fn vertex(v: Vertex) -> VertexOutput {
    var out: VertexOutput;
    let world_from_local = mesh_functions::get_world_from_local(v.instance_index);
    out.world_position = mesh_functions::mesh_position_local_to_world(world_from_local, vec4(v.position, 1.0));
    out.position = position_world_to_clip(out.world_position.xyz);
    let chunk = vec2<i32>(i32(v.packed.y & 0xFFFFu), i32(v.packed.y >> 16u));
    if textureLoad(mask, chunk, 0).r > 0.5 {
        out.position = vec4(0.0, 0.0, 0.0, 1.0);
    }
    let c = v.packed.x;
    out.color = vec4(vec3(f32(c & 255u), f32((c >> 8u) & 255u), f32((c >> 16u) & 255u)) / 255.0, 1.0);
#ifdef VERTEX_OUTPUT_INSTANCE_INDEX
    out.instance_index = v.instance_index;
#endif
    return out;
}
