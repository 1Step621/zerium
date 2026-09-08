fn encode_site(site: vec2<u32>) -> vec4<f32> {
    let x = site.x + 1u;
    let y = site.y + 1u;
    return vec4(
        f32(x & 255u),
        f32((x >> 8u) & 255u),
        f32(y & 255u),
        f32((y >> 8u) & 255u),
    ) / 255.0;
}

fn decode_site(value: vec4<f32>) -> vec2<i32> {
    let bytes = vec4<u32>(round(clamp(value, vec4(0.0), vec4(1.0)) * 255.0));
    let x = bytes.x | (bytes.y << 8u);
    let y = bytes.z | (bytes.w << 8u);
    if x == 0u || y == 0u {
        return vec2(-1);
    }
    return vec2(i32(x) - 1, i32(y) - 1);
}

fn source_alpha(position: vec2<i32>, size: vec2<i32>) -> f32 {
    if any(position < vec2(0)) || any(position >= size) {
        return 0.0;
    }
    return textureLoad(zerium_effect_source, position, 0).a;
}

fn is_boundary(position: vec2<i32>, size: vec2<i32>) -> bool {
    let alpha = source_alpha(position, size);
    if alpha <= 0.0 {
        return false;
    }
    if alpha < 1.0 {
        return true;
    }
    let offsets = array<vec2<i32>, 4>(
        vec2(-1, 0),
        vec2(1, 0),
        vec2(0, -1),
        vec2(0, 1),
    );
    for (var index = 0u; index < 4u; index += 1u) {
        if source_alpha(position + offsets[index], size) < 0.5 {
            return true;
        }
    }
    return false;
}

fn distance_squared(position: vec2<i32>, site: vec2<i32>, size: vec2<i32>) -> f32 {
    let delta = vec2<f32>(position - site);
    let edge_offset = 1.0 - source_alpha(site, size);
    return dot(delta, delta) + edge_offset * edge_offset;
}

fn search_radius(params: ZeriumParameters) -> i32 {
    // Include the one-pixel coverage ramp outside the nominal stroke width.
    return max(
        i32(ceil(max(params.width, 0.0) * zerium_render_context().composition_scale + 1.0)),
        1,
    );
}

@compute @workgroup_size(8, 8, 1)
fn compute_main(@builtin(global_invocation_id) global_id: vec3<u32>) {
    let output_size = vec2<u32>(zerium_render_context().output_size);
    if global_id.x >= output_size.x || global_id.y >= output_size.y {
        return;
    }

    let position = vec2<i32>(global_id.xy);
    let size = vec2<i32>(output_size);
    let params = zerium_load_parameters();

    if params.phase == 0u {
        if is_boundary(position, size) {
            zerium_store_output(global_id.xy, encode_site(global_id.xy));
        } else {
            zerium_store_output(global_id.xy, vec4(0.0));
        }
        return;
    }

    let radius = search_radius(params);
    var best_site = vec2(-1);
    var best_distance = 1.0e30;

    // Two bounded separable EDT passes. The requested effect radius bounds each
    // 1D envelope search without changing any distance inside the visible band.
    if params.phase == 1u {
        let first = max(position.y - radius, 0);
        let last = min(position.y + radius, size.y - 1);
        for (var candidate_y = first; candidate_y <= last; candidate_y += 1) {
            let candidate = decode_site(textureLoad(
                zerium_effect_input,
                vec2(position.x, candidate_y),
                0,
            ));
            if candidate.x < 0 {
                continue;
            }
            let distance = distance_squared(position, candidate, size);
            if distance < best_distance {
                best_distance = distance;
                best_site = candidate;
            }
        }
    } else {
        let first = max(position.x - radius, 0);
        let last = min(position.x + radius, size.x - 1);
        for (var candidate_x = first; candidate_x <= last; candidate_x += 1) {
            let candidate = decode_site(textureLoad(
                zerium_effect_input,
                vec2(candidate_x, position.y),
                0,
            ));
            if candidate.x < 0 {
                continue;
            }
            let distance = distance_squared(position, candidate, size);
            if distance < best_distance {
                best_distance = distance;
                best_site = candidate;
            }
        }
    }

    if best_site.x < 0 {
        zerium_store_output(global_id.xy, vec4(0.0));
    } else {
        zerium_store_output(global_id.xy, encode_site(vec2<u32>(best_site)));
    }
}
