import package::generated::host::render::{ZeriumVertex, vertex};
import package::generated::host::effect::{uv_to_position, input_position, sample_input, straight};
import package::generated::halftone::props;
import package::generated::support::util::{rotate, DEG2RAD, scene_color};
import package::helpers::geometry::distance_coverage;
import package::helpers::color::{luminance, display_rgb};

@vertex
fn vertex_main(@builtin(vertex_index) index: u32) -> ZeriumVertex {
    return vertex(index);
}

@fragment
fn fragment_main(input: ZeriumVertex) -> @location(0) vec4<f32> {
    let p = props();
    let position = uv_to_position(input.uv);
    let source = sample_input(position);
    let local = rotate(position - input_position(vec2(0.0)), -p.angle * DEG2RAD);
    let cell = floor(local / p.size);
    let center = (cell + vec2(0.5)) * p.size;
    let sampled = sample_input(input_position(vec2(0.0)) + rotate(center, p.angle * DEG2RAD));
    let darkness = 1.0 - clamp(luminance(display_rgb(straight(sampled), false)), 0.0, 1.0);
    let delta = local - center;
    var distance = length(delta) - sqrt(darkness) * p.size * 0.70710678;
    if p.mode == 1u {
        distance = abs(delta.y) - darkness * p.size * 0.5;
    }
    var coverage = distance_coverage(distance, 0.001);
    if darkness <= 0.0 {
        coverage = 0.0;
    }
    if darkness >= 1.0 {
        coverage = 1.0;
    }
    let output = mix(scene_color(p.paper), scene_color(p.ink), coverage) * source.a;
    return mix(source, output, p.amount / 100.0);
}
