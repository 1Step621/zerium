import package::generated::host::render::{ZeriumVertex, vertex};
import package::generated::host::effect::{uv_to_position, sample_input};
import package::generated::opacity::props;

@vertex
fn vertex_main(@builtin(vertex_index) vertex_index: u32) -> ZeriumVertex {
    return vertex(vertex_index);
}

@fragment
fn fragment_main(input: ZeriumVertex) -> @location(0) vec4<f32> {
    let properties = props();
    let amount = clamp(properties.opacity / 100.0, 0.0, 1.0);
    return sample_input(uv_to_position(input.uv)) * amount;
}
