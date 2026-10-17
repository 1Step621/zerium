import package::generated::host::effect::{context, pixel_position, load_input, load_source};
import package::generated::host::compute::store;
import package::generated::outline::{ZeriumProps, props};
import package::helpers::outline_sites::{decode_site, encode_site};
override phase: u32 = 0u;

fn source_alpha(position: vec2<i32>, size: vec2<i32>) -> f32 {
    if any(position < vec2(0)) || any(position >= size) {
        return 0.0;
    }

    return load_source(pixel_position(position)).a;
}

fn is_boundary(position: vec2<i32>, size: vec2<i32>) -> bool {
    let alpha = source_alpha(position, size);

    if alpha <= 0.0 {
        return false;
    }

    let offsets = array<vec2<i32>, 4>(vec2(-1, 0), vec2(1, 0), vec2(0, -1), vec2(0, 1));

    for(var index = 0u; index < 4u; index += 1u) {
        // Opacity is not coverage: a uniformly translucent interior is not an
        // edge. Seed the boundary of the nonzero silhouette instead.

        if source_alpha(position + offsets[index], size) <= 0.0 {
            return true;
        }
    }

    return false;
}

fn distance_squared(position: vec2<i32>, site: vec2<i32>) -> f32 {
    let delta = vec2<f32>(position - site);
    return dot(delta, delta);
}

fn search_radius(properties: ZeriumProps) -> i32 {
    // Include the one-pixel coverage ramp outside the nominal stroke width.
    return max(i32(ceil(max(properties.width, 0.0) * context().composition_scale + 1.0)), 1);
}

@compute @workgroup_size(8, 8, 1)
fn compute_main(@builtin(global_invocation_id) global_id: vec3<u32>) {
    let output_size = vec2<u32>(context().output_size);

    if global_id.x >= output_size.x || global_id.y >= output_size.y {
        return;
    }

    let position = vec2<i32>(global_id.xy);
    let size = vec2<i32>(output_size);
    let properties = props();

    if phase == 0u {
        if is_boundary(position, size) {
            store(global_id.xy, encode_site(global_id.xy));
        } else {
            store(global_id.xy, vec4(0.0));
        }

        return;
    }

    let radius = search_radius(properties);
    var best_site = vec2(-1);
    var best_distance = 1.0e30;

    // Two bounded separable EDT passes. The requested effect radius bounds each
    // 1D envelope search without changing any distance inside the visible band.

    if phase == 1u {
        let first = max(position.y - radius, 0);
        let last = min(position.y + radius, size.y - 1);

        for(var candidate_y = first; candidate_y <= last; candidate_y += 1) {
            let candidate = decode_site(load_input(pixel_position(vec2(position.x, candidate_y))));

            if candidate.x < 0 {
                continue;
            }

            let distance = distance_squared(position, candidate);

            if distance < best_distance {
                best_distance = distance;
                best_site = candidate;
            }
        }
    } else {
        let first = max(position.x - radius, 0);
        let last = min(position.x + radius, size.x - 1);

        for(var candidate_x = first; candidate_x <= last; candidate_x += 1) {
            let candidate = decode_site(load_input(pixel_position(vec2(candidate_x, position.y))));

            if candidate.x < 0 {
                continue;
            }

            let distance = distance_squared(position, candidate);

            if distance < best_distance {
                best_distance = distance;
                best_site = candidate;
            }
        }
    }

    if best_site.x < 0 {
        store(global_id.xy, vec4(0.0));
    } else {
        store(global_id.xy, encode_site(vec2<u32>(best_site)));
    }
}
