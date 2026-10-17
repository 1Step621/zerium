import package::generated::host::render::{ZeriumVertex, vertex};
import package::generated::host::effect::{uv_to_position, input_position, sample_input};
import package::generated::scale::props;

@vertex
fn vertex_main(@builtin(vertex_index) vertex_index: u32) -> ZeriumVertex {
    return vertex(vertex_index);
}

@fragment
fn fragment_main(input: ZeriumVertex) -> @location(0) vec4<f32> {
    let properties = props();
    let factor = vec2(properties.amount.v0, properties.amount.v1) / 100.0;

    if any(factor == vec2(0.0)) {
        return vec4(0.0);
    }

    let center = input_position(vec2(0.5));
    let source_position = center + (uv_to_position(input.uv) - center) / factor;
    return sample_input(source_position);
}
