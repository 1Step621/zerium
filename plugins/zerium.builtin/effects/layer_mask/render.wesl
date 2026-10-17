import package::generated::host::render::{ZeriumVertex, vertex};
import package::generated::host::effect::{uv_to_position, sample_input};
import package::generated::layer_mask::props;
import package::generated::layer_mask::inputs::behind::sample;

@vertex
fn vertex_main(@builtin(vertex_index) vertex_index: u32) -> ZeriumVertex {
    return vertex(vertex_index);
}

@fragment
fn fragment_main(input: ZeriumVertex) -> @location(0) vec4<f32> {
    let layers = sample(uv_to_position(input.uv));
    let alpha = sample_input(uv_to_position(input.uv)).a;
    let mask = select(alpha, 1.0 - alpha, props().invert_mask);
    return layers * mask;
}
