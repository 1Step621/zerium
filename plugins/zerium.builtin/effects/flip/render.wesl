import package::generated::host::render::{ZeriumVertex, vertex};
import package::generated::host::effect::{uv_to_position, input_position, sample_input};
import package::generated::flip::props;

@vertex
fn vertex_main(@builtin(vertex_index) vertex_index: u32) -> ZeriumVertex {
    return vertex(vertex_index);
}

@fragment
fn fragment_main(input: ZeriumVertex) -> @location(0) vec4<f32> {
    let properties = props();
    let center = input_position(vec2(0.5));
    let sign = select(vec2(1.0), vec2(-1.0), vec2(properties.horizontal, properties.vertical));
    return sample_input(center + (uv_to_position(input.uv) - center) * sign);
}
