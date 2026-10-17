import package::generated::host::render::{ZeriumVertex, vertex};
import package::generated::host::effect::{uv_to_position, straight, sample_input};
import package::generated::support::util::scene_color;
import package::generated::gradient_map::{get_stops, props};
import package::helpers::color::luminance;
import package::helpers::gradient::{gradient_factor, gradient_include, gradient_start};

@vertex
fn vertex_main(@builtin(vertex_index) vertex_index: u32) -> ZeriumVertex {
    return vertex(vertex_index);
}

@fragment
fn fragment_main(input: ZeriumVertex) -> @location(0) vec4<f32> {
    let properties = props();
    let source = sample_input(uv_to_position(input.uv));

    if properties.amount <= 0.0 {
        return source;
    }

    let brightness = clamp(luminance(straight(source)), 0.0, 1.0);
    let position = brightness * 100.0;
    let first = get_stops(properties, 0u);
    var segment = gradient_start(first.v0, first.v1);

    for(var index = 1u; index < properties.stops_len; index += 1u) {
        let stop = get_stops(properties, index);
        segment = gradient_include(segment, stop.v0, stop.v1, position);
    }

    let mapped = mix(
        scene_color(segment.start_color),
        scene_color(segment.end_color),
        gradient_factor(segment, position),
    ) * source.a;
    return mix(source, mapped, properties.amount / 100.0);
}
