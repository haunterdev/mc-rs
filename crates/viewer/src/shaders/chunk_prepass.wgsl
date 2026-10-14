// Depth, normal and motion-vector prepass for chunks; also the shadow pass. Cutout texels are
// discarded here the same way as in the main pass.
#import bevy_pbr::{
    prepass_io::{VertexOutput, FragmentOutput},
    prepass_bindings,
    mesh_view_bindings::view,
}
#import viewer::chunk_common::{tag_of, sample_block}

#ifdef PREPASS_FRAGMENT
@fragment
fn fragment(in: VertexOutput) -> FragmentOutput {
#ifdef MAY_DISCARD
    if sample_block(in.uv, tag_of(in.color)).a <= 0.5 {
        discard;
    }
#endif
    var out: FragmentOutput;
#ifdef NORMAL_PREPASS
    out.normal = vec4(in.world_normal * 0.5 + vec3(0.5), 1.0);
#endif
#ifdef UNCLIPPED_DEPTH_ORTHO_EMULATION
    out.frag_depth = in.unclipped_depth;
#endif
#ifdef MOTION_VECTOR_PREPASS
    let clip = view.unjittered_clip_from_world * in.world_position;
    let prev = prepass_bindings::previous_view_uniforms.clip_from_world * in.previous_world_position;
    out.motion_vector = (clip.xy / clip.w - prev.xy / prev.w) * vec2(0.5, -0.5);
#endif
    return out;
}
#else
@fragment
fn fragment(in: VertexOutput) {
#ifdef MAY_DISCARD
    if sample_block(in.uv, tag_of(in.color)).a <= 0.5 {
        discard;
    }
#endif
}
#endif
