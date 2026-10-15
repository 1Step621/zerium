import package::generated::host::effect::{
    ZeriumVertex,
    effect_input,
    effect_sampler,
    straight,
    vertex,
};
import package::generated::host::util::srgb;
import package::generated::flip::props;
import package::helpers::color::encode_srgb;

@vertex
fn vertex_main(@builtin(vertex_index) vertex_index: u32) -> ZeriumVertex {
    return vertex(vertex_index);
}

@fragment
fn fragment_main(input: ZeriumVertex) -> @location(0) vec4<f32> {
    let properties = props();
    let uv = select(input.uv, vec2(1.0) - input.uv, vec2(properties.horizontal, properties.vertical));
    let source = textureSample(effect_input, effect_sampler, uv);
    if !properties.invert_color {
        return source;
    }
    let inverted = vec3(1.0) - clamp(encode_srgb(straight(source)), vec3(0.0), vec3(1.0));
    return vec4(srgb(inverted) * source.a, source.a);
}
