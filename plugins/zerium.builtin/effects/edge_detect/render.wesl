import package::generated::host::render::{ZeriumVertex, vertex};
import package::generated::host::effect::{uv_to_position, sample_input};
import package::generated::edge_detect::props;
import package::generated::host::effect::over;
import package::generated::support::util::scene_color;
import package::helpers::color::luminance;

fn edge_value(color: vec4<f32>) -> vec2<f32> {
    return vec2(luminance(color.rgb), color.a);
}

@vertex
fn vertex_main(@builtin(vertex_index) index: u32) -> ZeriumVertex {
    return vertex(index);
}

@fragment
fn fragment_main(input: ZeriumVertex) -> @location(0) vec4<f32> {
    let p = props();
    let position = uv_to_position(input.uv);
    let source = sample_input(position);
    let dx = vec2(p.width, 0.0);
    let dy = vec2(0.0, p.width);
    let a = edge_value(sample_input(position - dx - dy));
    let b = edge_value(sample_input(position - dy));
    let c = edge_value(sample_input(position + dx - dy));
    let d = edge_value(sample_input(position - dx));
    let f = edge_value(sample_input(position + dx));
    let g = edge_value(sample_input(position - dx + dy));
    let h = edge_value(sample_input(position + dy));
    let i = edge_value(sample_input(position + dx + dy));
    let gx = -a - 2.0 * d - g + c + 2.0 * f + i;
    let gy = -a - 2.0 * b - c + g + 2.0 * h + i;
    let strength = clamp(
        max(length(vec2(gx.x, gy.x)), length(vec2(gx.y, gy.y))) * p.strength / 400.0,
        0.0,
        1.0,
    );
    let edge = scene_color(p.color) * strength;
    return over(edge, scene_color(p.background) * source.a);
}
