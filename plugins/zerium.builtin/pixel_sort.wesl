import package::generated::compute::{effect_input, context, store};
import package::generated::properties_pixel_sort::props;
import package::generated::effect_bounds::{effect_bounds, contains};

fn pixel_luminance(color: vec4<f32>) -> f32 {
    return dot(color.rgb, vec3(0.2126, 0.7152, 0.0722));
}

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
            || (candidate_luminance == current_luminance
                && candidate_index < current_index);
    }
    return candidate_luminance < current_luminance
        || (candidate_luminance == current_luminance
            && candidate_index < current_index);
}

@compute @workgroup_size(8, 8, 1)
fn compute_main(@builtin(global_invocation_id) global_id: vec3<u32>) {
    let output_size = vec2<u32>(context().output_size);
    if global_id.x >= output_size.x || global_id.y >= output_size.y {
        return;
    }

    let properties = props();
    let current_position = global_id.xy;
    let color = textureLoad(effect_input, vec2<i32>(current_position), 0);
    if !contains(current_position, output_size, 0.0) {
        store(current_position, color);
        return;
    }

    let axis = select(current_position.x, current_position.y, properties.vertical);
    let line = select(current_position.y, current_position.x, properties.vertical);
    let logical_start = select(
        output_size.x - effect_bounds.from_right,
        output_size.y - effect_bounds.from_bottom,
        properties.vertical,
    );
    let logical_end = select(effect_bounds.max_x, effect_bounds.max_y, properties.vertical);
    let current_luminance = pixel_luminance(color);
    if current_luminance < properties.threshold {
        store(current_position, color);
        return;
    }

    var segment_start = axis;
    while segment_start > logical_start {
        let candidate_position = axis_position(segment_start - 1u, line, properties.vertical);
        let candidate = textureLoad(effect_input, vec2<i32>(candidate_position), 0);
        if pixel_luminance(candidate) < properties.threshold {
            break;
        }
        segment_start -= 1u;
    }
    var segment_end = axis + 1u;
    while segment_end < logical_end {
        let candidate_position = axis_position(segment_end, line, properties.vertical);
        let candidate = textureLoad(effect_input, vec2<i32>(candidate_position), 0);
        if pixel_luminance(candidate) < properties.threshold {
            break;
        }
        segment_end += 1u;
    }

    var rank = 0u;
    for (var candidate_axis = segment_start; candidate_axis < segment_end; candidate_axis += 1u) {
        let candidate_position = axis_position(candidate_axis, line, properties.vertical);
        let candidate = textureLoad(effect_input, vec2<i32>(candidate_position), 0);
        if candidate_precedes(
            pixel_luminance(candidate),
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
