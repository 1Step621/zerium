import package::generated::host::render::{ZeriumVertex, vertex};
import package::generated::host::effect::{uv_to_position, input_position, sample_input};
import package::generated::fisheye::props;

@vertex
fn vertex_main(@builtin(vertex_index) vertex_index: u32) -> ZeriumVertex {
    return vertex(vertex_index);
}

@fragment
fn fragment_main(input: ZeriumVertex) -> @location(0) vec4<f32> {
    let properties = props();

    if properties.strength == 0.0 {
        return sample_input(uv_to_position(input.uv));
    }

    let size = input_position(vec2(1.0)) - input_position(vec2(0.0));
    let center = vec2(properties.center.v0, properties.center.v1);
    let radius = min(size.x, size.y) * 0.5 * properties.radius / 100.0;
    let position = uv_to_position(input.uv);
    let local = (position - center) / radius;
    let distance = length(local);
    var source_position = position;

    if distance > 0.00001 && distance < 1.0 {
        let mapped = pow(distance, exp2(properties.strength / 100.0 * 1.5));
        source_position = center + local * (mapped / distance) * radius;
    }

    return sample_input(source_position);
}
