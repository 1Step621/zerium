import package::generated::host::render::{ZeriumVertex, vertex};
import package::generated::host::effect::{
    uv_to_position,
    uv_inside,
    input_info,
    input_position,
    input_reference_uv,
    sample_input
};
import package::generated::support::util::DEG2RAD;
import package::generated::polar_coordinates::props;
import package::helpers::geometry::TAU;

@vertex
fn vertex_main(@builtin(vertex_index) vertex_index: u32) -> ZeriumVertex {
    return vertex(vertex_index);
}

@fragment
fn fragment_main(input: ZeriumVertex) -> @location(0) vec4<f32> {
    let properties = props();
    let position = uv_to_position(input.uv);
    let size = input_info().bounds_size;

    if any(size <= vec2(0.0)) {
        return vec4(0.0);
    }

    let original = input_reference_uv(position);

    if !uv_inside(original) {
        return vec4(0.0);
    }

    if properties.amount <= 0.0 {
        return sample_input(position);
    }

    let center = input_position(vec2(0.5));
    let radius = min(size.x, size.y) * 0.5;
    var mapped = position;

    if properties.direction == 0u {
        let local = (position - center) / radius;
        mapped = input_position(
            vec2(
                fract(atan2(local.y, local.x) / TAU - properties.angle / 360.0 + 1.0),
                length(local),
            ),
        );
    } else {
        let angle = original.x * TAU + properties.angle * DEG2RAD;
        mapped = center + vec2(cos(angle), sin(angle)) * original.y * radius;
    }

    return sample_input(mix(position, mapped, properties.amount / 100.0));
}
