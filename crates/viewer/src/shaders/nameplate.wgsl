// Name tags (players.rs, EntityRenderer.drawNameplate): the background quad in a flat color, or
// the font texel times the color, alpha tested at 0.1 as the font renderer leaves it.
#import bevy_pbr::forward_io::VertexOutput
#import bevy_render::color_operations::srgb_to_linear

struct NameplateParams {
    color: vec4<f32>,
    // x: 1 for the background quad.
    mode: vec4<f32>,
}

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var font: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(1) var font_sampler: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(2) var<uniform> params: NameplateParams;

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    if params.mode.x > 0.5 {
        return vec4(srgb_to_linear(params.color.rgb), params.color.a);
    }
    let tex = textureSample(font, font_sampler, in.uv);
    if tex.a <= 0.1 {
        discard;
    }
    return vec4(srgb_to_linear(tex.rgb * params.color.rgb), tex.a * params.color.a);
}
