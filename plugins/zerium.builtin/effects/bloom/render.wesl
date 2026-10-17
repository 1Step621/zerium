import package::generated::host::render::{ZeriumVertex, vertex};
import package::generated::host::effect::{uv_to_position, sample_input, sample_source};
import package::generated::bloom::props;

@vertex
fn vertex_main(@builtin(vertex_index) vertex_index: u32) -> ZeriumVertex {
    return vertex(vertex_index);
}

@fragment
fn fragment_main(input: ZeriumVertex) -> @location(0) vec4<f32> {
    let properties = props();
    let source = sample_source(uv_to_position(input.uv));

    if properties.radius <= 0.0 || properties.intensity <= 0.0 {
        return source;
    }

    let light = sample_input(uv_to_position(input.uv)) * properties.intensity / 100.0;
    let alpha = source.a + clamp(light.a, 0.0, 1.0) * (1.0 - source.a);
    return vec4(source.rgb + light.rgb, alpha);
}
