import package::generated::host::render::{ZeriumVertex, vertex};
import package::generated::host::effect::{uv_to_position, straight, sample_input};
import package::generated::support::util::srgb;
import package::generated::invert_colors::props;
import package::helpers::color::encode_srgb;

@vertex
fn vertex_main(@builtin(vertex_index) vertex_index: u32) -> ZeriumVertex {
    return vertex(vertex_index);
}

@fragment
fn fragment_main(input: ZeriumVertex) -> @location(0) vec4<f32> {
    let amount = props().amount / 100.0;
    let source = sample_input(uv_to_position(input.uv));

    if amount <= 0.0 {
        return source;
    }

    let encoded = clamp(encode_srgb(straight(source)), vec3(0.0), vec3(1.0));
    let inverted = vec4(srgb(vec3(1.0) - encoded) * source.a, source.a);
    return mix(source, inverted, amount);
}
