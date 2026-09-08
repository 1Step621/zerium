const MAX_BLOCK_SIZE: u32 = 256u;

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
            || candidate_luminance == current_luminance
                && candidate_index < current_index;
    }
    return candidate_luminance < current_luminance
        || candidate_luminance == current_luminance
            && candidate_index < current_index;
}

@compute @workgroup_size(8, 8, 1)
fn compute_main(@builtin(global_invocation_id) global_id: vec3<u32>) {
    let output_size = vec2<u32>(zerium_render_context().output_size);
    if global_id.x >= output_size.x || global_id.y >= output_size.y {
        return;
    }

    let params = zerium_load_parameters();
    let block_size = clamp(
        u32(round(f32(params.block_size) * zerium_render_context().composition_scale)),
        2u,
        MAX_BLOCK_SIZE,
    );
    let current_position = global_id.xy;
    let axis = select(current_position.x, current_position.y, params.vertical);
    let line = select(current_position.y, current_position.x, params.vertical);
    let axis_size = select(output_size.x, output_size.y, params.vertical);
    let color = textureLoad(zerium_effect_input, vec2<i32>(current_position), 0);
    let current_luminance = pixel_luminance(color);
    if current_luminance < params.threshold {
        zerium_store_output(current_position, color);
        return;
    }

    let logical_start = axis / block_size * block_size;
    let logical_end = min(logical_start + block_size, axis_size);
    var segment_start = axis;
    while segment_start > logical_start {
        let candidate_position = axis_position(segment_start - 1u, line, params.vertical);
        let candidate = textureLoad(zerium_effect_input, vec2<i32>(candidate_position), 0);
        if pixel_luminance(candidate) < params.threshold {
            break;
        }
        segment_start -= 1u;
    }
    var segment_end = axis + 1u;
    while segment_end < logical_end {
        let candidate_position = axis_position(segment_end, line, params.vertical);
        let candidate = textureLoad(zerium_effect_input, vec2<i32>(candidate_position), 0);
        if pixel_luminance(candidate) < params.threshold {
            break;
        }
        segment_end += 1u;
    }

    var rank = 0u;
    for (var candidate_axis = segment_start; candidate_axis < segment_end; candidate_axis += 1u) {
        let candidate_position = axis_position(candidate_axis, line, params.vertical);
        let candidate = textureLoad(zerium_effect_input, vec2<i32>(candidate_position), 0);
        if candidate_precedes(
            pixel_luminance(candidate),
            candidate_axis,
            current_luminance,
            axis,
            params.descending,
        ) {
            rank += 1u;
        }
    }

    let destination = axis_position(segment_start + rank, line, params.vertical);
    zerium_store_output(destination, color);
}
