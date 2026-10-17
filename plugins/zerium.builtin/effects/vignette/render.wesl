import package::generated::host::render::{ZeriumVertex, vertex};
import package::generated::host::effect::{
    uv_to_position,
    input_info,
    input_reference_uv,
    sample_input
};
import package::generated::vignette::props;

@vertex
fn vertex_main(@builtin(vertex_index) vertex_index: u32) -> ZeriumVertex {
    return vertex(vertex_index);
}

@fragment
fn fragment_main(input: ZeriumVertex) -> @location(0) vec4<f32> {
    let properties = props();
    let dimensions = input_info().bounds_size;
    let aspect = dimensions.x / max(dimensions.y, 1.0);
    var centered = input_reference_uv(uv_to_position(input.uv)) * 2.0 - vec2(1.0);
    centered.x *= aspect * max(properties.roundness, 0.01);
    let corner_distance = length(vec2(aspect * max(properties.roundness, 0.01), 1.0));
    let distance = length(centered) / max(corner_distance, 0.00001);
    let vignette = smoothstep(max(1.0 - properties.softness, 0.0), 1.0, distance)
        * clamp(properties.amount, 0.0, 1.0);
    let source = sample_input(uv_to_position(input.uv));
    return vec4(source.rgb * (1.0 - vignette), source.a);
}
