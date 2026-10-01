import package::generated::host::effect::{ZeriumVertex, vertex, uv_to_position, input_uv_to_position, position_to_uv, sample_or_transparent};
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
    let center = input_uv_to_position(vec2(0.5));
    let source_position = center + (uv_to_position(input.uv) - center) / factor;
    return sample_or_transparent(position_to_uv(source_position));
}
