import package::generated::host::render::{ZeriumVertex, vertex};
import package::generated::host::effect::{uv_to_position, straight, sample_input};
import package::generated::support::util::{DEG2RAD, srgb};
import package::generated::emboss::props;
import package::helpers::color::luminance;

@vertex
fn vertex_main(@builtin(vertex_index) vertex_index: u32) -> ZeriumVertex {
    return vertex(vertex_index);
}

@fragment
fn fragment_main(input: ZeriumVertex) -> @location(0) vec4<f32> {
    let properties = props();
    let direction = vec2(cos(properties.angle * DEG2RAD), sin(properties.angle * DEG2RAD));
    let offset = direction * properties.distance;
    let source = sample_input(uv_to_position(input.uv));
    let a = sample_input(uv_to_position(input.uv) - offset);
    let b = sample_input(uv_to_position(input.uv) + offset);
    let slope = luminance(a.rgb - b.rgb) * properties.strength / 100.0;
    let gray = srgb(vec3(clamp(0.5 + slope, 0.0, 1.0)));
    let colored = straight(source) * max(1.0 + slope, 0.0);
    return vec4(select(gray, colored, properties.preserve_color) * source.a, source.a);
}
