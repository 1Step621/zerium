import package::generated::host::render::{ZeriumVertex, vertex};
import package::generated::host::effect::{uv_to_position, sample_input};
import package::generated::light_sweep::props;
import package::generated::support::util::{DEG2RAD, scene_color};

@vertex
fn vertex_main(@builtin(vertex_index) index: u32) -> ZeriumVertex {
    return vertex(index);
}

@fragment
fn fragment_main(input: ZeriumVertex) -> @location(0) vec4<f32> {
    let p = props();
    let position = uv_to_position(input.uv);
    let source = sample_input(position);
    let direction = vec2(cos(p.angle * DEG2RAD), sin(p.angle * DEG2RAD));
    let distance = abs(dot(position - vec2(p.position.v0, p.position.v1), direction));
    let width = max(p.width * 0.5, 0.0001);
    let softness = max(width * p.softness / 100.0, 0.5);
    let band = 1.0 - smoothstep(max(width - softness, 0.0), width, distance);
    let tint = scene_color(p.color);
    return vec4(source.rgb + tint.rgb * source.a * band * p.intensity / 100.0, source.a);
}
