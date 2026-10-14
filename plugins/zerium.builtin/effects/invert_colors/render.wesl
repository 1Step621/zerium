import package::generated::host::effect::{
    ZeriumVertex, effect_input, effect_sampler, straight, vertex,
};
import package::generated::host::util::srgb;
import package::generated::invert_colors::props;
import package::shared::color::encode_srgb;

@vertex
fn vertex_main(@builtin(vertex_index) vertex_index: u32) -> ZeriumVertex {
    return vertex(vertex_index);
}

@fragment
fn fragment_main(input: ZeriumVertex) -> @location(0) vec4<f32> {
    let amount = props().amount / 100.0;
    let source = textureSample(effect_input, effect_sampler, input.uv);
    if amount <= 0.0 {
        return source;
    }
    let encoded = clamp(encode_srgb(straight(source)), vec3(0.0), vec3(1.0));
    let inverted = vec4(srgb(vec3(1.0) - encoded) * source.a, source.a);
    return mix(source, inverted, amount);
}
