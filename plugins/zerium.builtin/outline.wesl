import package::generated::effect::{effect_input, effect_sampler, effect_source, context, ZeriumVertex, vertex, over, premul};
import package::generated::properties_outline::props;

@vertex
fn vertex_main(@builtin(vertex_index) vertex_index: u32) -> ZeriumVertex {
    return vertex(vertex_index);
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

fn source_sample(sample_position: vec2<f32>, dimensions: vec2<i32>) -> vec4<f32> {
    let uv = clamp(
        (sample_position + vec2(0.5)) / vec2<f32>(dimensions),
        vec2(0.0),
        vec2(1.0),
    );
    return textureSampleLevel(effect_source, effect_sampler, uv, 0.0);
}

fn distance_at(
    sample_position: vec2<f32>,
    position: vec2<i32>,
    dimensions: vec2<i32>,
) -> f32 {
    var best_distance_squared = 1.0e30;
    for (var offset_y = -1; offset_y <= 1; offset_y += 1) {
        for (var offset_x = -1; offset_x <= 1; offset_x += 1) {
            let candidate_position = clamp(
                position + vec2(offset_x, offset_y),
                vec2(0),
                dimensions - vec2(1),
            );
            let site = decode_site(textureLoad(effect_input, candidate_position, 0));
            if site.x < 0 {
                continue;
            }
            let site_alpha = textureLoad(effect_source, site, 0).a;
            let delta = sample_position - vec2<f32>(site);
            let edge_offset = 1.0 - site_alpha;
            best_distance_squared = min(
                best_distance_squared,
                dot(delta, delta) + edge_offset * edge_offset,
            );
        }
    }
    return sqrt(best_distance_squared);
}

@fragment
fn fragment_main(input: ZeriumVertex) -> @location(0) vec4<f32> {
    let properties = props();
    let dimensions = vec2<i32>(textureDimensions(effect_input));
    let position = clamp(
        vec2<i32>(input.uv * vec2<f32>(dimensions)),
        vec2(0),
        dimensions - vec2(1),
    );
    let source_pixel = textureLoad(effect_source, position, 0);
    if properties.width <= 0.0 {
        return source_pixel;
    }
    // The bounded distance transform stores no site away from the stroke.
    // Most of the full-frame pixels can reuse the source without subpixel work.
    if decode_site(textureLoad(effect_input, position, 0)).x < 0 {
        return source_pixel;
    }

    let composition_scale = context().composition_scale;
    let logical_radius = select(properties.width, properties.width * 0.5, properties.position == 1u);
    let radius = logical_radius * composition_scale;

    // Two samples per axis preserve diagonal coverage at the effect's 2x
    // render scale without evaluating the 3x3 distance neighborhood 16 times.
    var result = vec4(0.0);
    for (var sample_y = 0u; sample_y < 2u; sample_y += 1u) {
        for (var sample_x = 0u; sample_x < 2u; sample_x += 1u) {
            let offset = (vec2(f32(sample_x), f32(sample_y)) + vec2(0.5)) / 2.0
                - vec2(0.5);
            let sample_position = vec2<f32>(position) + offset;
            let source = source_sample(sample_position, dimensions);
            let distance_pixels = distance_at(sample_position, position, dimensions);
            let coverage = clamp(radius + 1.0 - distance_pixels, 0.0, 1.0);
            let outside = coverage * (1.0 - source.a);
            let inside = coverage * source.a;

            if properties.position == 0u {
                result += over(
                    source,
                    premul(properties.color, outside),
                );
            } else if properties.position == 2u {
                result += over(
                    premul(properties.color, inside),
                    source,
                );
            } else {
                let outside_result = over(
                    source,
                    premul(properties.color, outside),
                );
                result += over(
                    premul(properties.color, inside),
                    outside_result,
                );
            }
        }
    }
    return result / 4.0;
}
