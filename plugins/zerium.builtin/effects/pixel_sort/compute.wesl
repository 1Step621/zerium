import package::generated::host::effect::{context, pixel_position, load_input};
import package::generated::host::compute::store;
import package::generated::pixel_sort::props;
import package::helpers::color::luminance;

fn axis_position(axis: u32, line: u32, vertical: bool) -> vec2<u32> {
    return select(vec2(axis, line), vec2(line, axis), vertical);
}

fn candidate_precedes(
    candidate_luminance: f32,
    candidate_index: u32,
    current_luminance: f32,
    current_index: u32,
    descending: bool,
) -> bool {
    if descending {
        return candidate_luminance > current_luminance
            || (candidate_luminance == current_luminance && candidate_index < current_index);
    }

    return candidate_luminance < current_luminance
        || (candidate_luminance == current_luminance && candidate_index < current_index);
}

@compute @workgroup_size(8, 8, 1)
fn compute_main(@builtin(global_invocation_id) global_id: vec3<u32>) {
    let output_size = vec2<u32>(context().output_size);

    if global_id.x >= output_size.x || global_id.y >= output_size.y {
        return;
    }

    let properties = props();
    let current_position = global_id.xy;
    let color = load_input(pixel_position(vec2<i32>(current_position)));
    let axis = select(current_position.x, current_position.y, properties.vertical);
    let line = select(current_position.y, current_position.x, properties.vertical);
    let logical_start = 0u;
    let logical_end = select(output_size.x, output_size.y, properties.vertical);
    let current_luminance = luminance(color.rgb);

    if current_luminance < properties.threshold {
        store(current_position, color);
        return;
    }

    var segment_start = axis;
    while segment_start > logical_start {
        let candidate_position = axis_position(segment_start - 1u, line, properties.vertical);
        let candidate = load_input(pixel_position(vec2<i32>(candidate_position)));

        if luminance(candidate.rgb) < properties.threshold {
            break;
        }

        segment_start -= 1u;
    }

    var segment_end = axis + 1u;
    while segment_end < logical_end {
        let candidate_position = axis_position(segment_end, line, properties.vertical);
        let candidate = load_input(pixel_position(vec2<i32>(candidate_position)));

        if luminance(candidate.rgb) < properties.threshold {
            break;
        }

        segment_end += 1u;
    }

    var rank = 0u;

    for(var candidate_axis = segment_start; candidate_axis < segment_end; candidate_axis += 1u) {
        let candidate_position = axis_position(candidate_axis, line, properties.vertical);
        let candidate = load_input(pixel_position(vec2<i32>(candidate_position)));

        if candidate_precedes(
            luminance(candidate.rgb),
            candidate_axis,
            current_luminance,
            axis,
            properties.descending,
        ) {
            rank += 1u;
        }
    }

    let destination = axis_position(segment_start + rank, line, properties.vertical);
    store(destination, color);
}
