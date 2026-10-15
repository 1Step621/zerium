import package::generated::host::effect::{
    ZeriumVertex, effect_input, effect_sampler, straight, vertex,
};
import package::generated::host::util::srgb;
import package::generated::levels::props;
import package::helpers::color::encode_srgb;

@vertex
fn vertex_main(@builtin(vertex_index) vertex_index: u32) -> ZeriumVertex {
    return vertex(vertex_index);
}

@fragment
fn fragment_main(input: ZeriumVertex) -> @location(0) vec4<f32> {
    let properties = props();
    let source = textureSample(effect_input, effect_sampler, input.uv);
    if properties.input_black == 0.0 && properties.input_white == 100.0
        && properties.gamma == 1.0 && properties.output_black == 0.0 && properties.output_white == 100.0 {
        return source;
    }
    let encoded = encode_srgb(straight(source));
    let black = properties.input_black / 100.0;
    let width = max((properties.input_white - properties.input_black) / 100.0, 0.000001);
    let normalized = clamp((encoded - vec3(black)) / width, vec3(0.0), vec3(1.0));
    let corrected = pow(normalized, vec3(1.0 / properties.gamma));
    let output = mix(vec3(properties.output_black / 100.0), vec3(properties.output_white / 100.0), corrected);
    return vec4(srgb(output) * source.a, source.a);
}
