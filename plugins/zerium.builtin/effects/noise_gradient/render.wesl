import package::generated::host::render::{ZeriumVertex, vertex};
import package::generated::host::effect::{uv_to_position, input_info, input_position, sample_input};
import package::generated::support::util::{DEG2RAD, scene_color};
import package::generated::noise_gradient::{get_stops, props};
import package::helpers::gradient::{gradient_factor, gradient_include, gradient_start};
import package::helpers::noise::cell_hash;

@vertex
fn vertex_main(@builtin(vertex_index) vertex_index: u32) -> ZeriumVertex {
    return vertex(vertex_index);
}

@fragment
fn fragment_main(input: ZeriumVertex) -> @location(0) vec4<f32> {
    let properties = props();
    let source = sample_input(uv_to_position(input.uv));
    let size = input_info().bounds_size;
    let cell = vec2<i32>(
        floor((uv_to_position(input.uv) - input_position(vec2(0.0))) / properties.grain_size),
    );
    let point = (vec2<f32>(cell) + vec2(0.5)) * properties.grain_size - size * 0.5;
    let angle = properties.angle * DEG2RAD;
    let direction = vec2(cos(angle), sin(angle));
    let span = max(dot(abs(direction), size), 0.00001);
    let coverage = clamp(
        (dot(point, direction) / span - properties.offset / 100.0) * 100.0 / properties.width
        + 0.5,
        0.0,
        1.0,
    );
    let phase_seed = bitcast<u32>(i32(floor(properties.phase * 60.0)));
    let noise = cell_hash(cell, properties.seed + phase_seed * 1013u);
    let position = coverage * 100.0;
    let first = get_stops(properties, 0u);
    var segment = gradient_start(first.v0, first.v1);

    for(var index = 1u; index < properties.stops_len; index += 1u) {
        let stop = get_stops(properties, index);
        segment = gradient_include(segment, stop.v0, stop.v1, position);
    }

    // Each grain picks one of the neighboring stop colors; only its probability
    // changes through the transition, keeping the stippled appearance.
    let color = select(
        segment.start_color,
        segment.end_color,
        noise < gradient_factor(segment, position),
    );
    let gradient = scene_color(color) * source.a;
    return mix(source, gradient, properties.amount / 100.0);
}
