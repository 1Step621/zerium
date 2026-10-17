import package::generated::host::render::{ZeriumVertex, vertex};
import package::generated::host::effect::{uv_to_position, straight, sample_input};
import package::generated::support::util::DEG2RAD;
import package::generated::chromatic_aberration::props;

@vertex
fn vertex_main(@builtin(vertex_index) vertex_index: u32) -> ZeriumVertex {
    return vertex(vertex_index);
}

@fragment
fn fragment_main(input: ZeriumVertex) -> @location(0) vec4<f32> {
    let properties = props();
    let angle = properties.angle * DEG2RAD;
    let offset = vec2(cos(angle), sin(angle)) * properties.amount;
    let red_sample = sample_input(uv_to_position(input.uv) + offset);
    let center_sample = sample_input(uv_to_position(input.uv));
    let blue_sample = sample_input(uv_to_position(input.uv) - offset);
    let alpha = max(red_sample.a, max(center_sample.a, blue_sample.a));
    let color = vec3(straight(red_sample).r, straight(center_sample).g, straight(blue_sample).b);
    return vec4(color * alpha, alpha);
}
