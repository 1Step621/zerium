import package::generated::util::DEG2RAD;
import package::generated::effect::{ZeriumVertex, vertex, uv_to_position, position_to_uv, sample_or_transparent, rotate};
import package::generated::properties_rotate_2d::props;

@vertex
fn vertex_main(@builtin(vertex_index) vertex_index: u32) -> ZeriumVertex {
    return vertex(vertex_index);
}

@fragment
fn fragment_main(input: ZeriumVertex) -> @location(0) vec4<f32> {
    let properties = props();
    let center = vec2(properties.center.v0, properties.center.v1);
    let output_position = uv_to_position(input.uv);
    let relative_position = output_position - center;
    let angle = -properties.angle * DEG2RAD;
    let source_position = rotate(relative_position, angle) + center;
    let source_uv = position_to_uv(source_position);
    return sample_or_transparent(source_uv);
}
