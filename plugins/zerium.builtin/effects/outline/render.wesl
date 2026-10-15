import package::generated::host::effect::{
    ZeriumVertex,
    context,
    effect_input,
    effect_sampler,
    effect_source,
    premul,
    vertex,
};
import package::generated::outline::props;
import package::helpers::outline_sites::decode_site;

@vertex
fn vertex_main(@builtin(vertex_index) vertex_index: u32) -> ZeriumVertex {
    return vertex(vertex_index);
}

fn source_sample(sample_position: vec2<f32>, dimensions: vec2<i32>) -> vec4<f32> {
    let uv = clamp(
        (sample_position + vec2(0.5)) / vec2<f32>(dimensions),
        vec2(0.0),
        vec2(1.0),
    );
    return textureSampleLevel(effect_source, effect_sampler, uv, 0.0);
}

fn local_opacity(position: vec2<i32>, dimensions: vec2<i32>) -> f32 {
    var opacity = 0.0;
    for (var y = -1; y <= 1; y += 1) {
        for (var x = -1; x <= 1; x += 1) {
            let neighbor = clamp(position + vec2(x, y), vec2(0), dimensions - vec2(1));
            opacity = max(opacity, textureLoad(effect_source, neighbor, 0).a);
        }
    }
    return opacity;
}

fn distance_at(
    sample_position: vec2<f32>,
    position: vec2<i32>,
    dimensions: vec2<i32>,
    opacity: f32,
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
            let edge_offset = 1.0 - clamp(site_alpha / opacity, 0.0, 1.0);
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
    if properties.width <= 0.0 || properties.color.a <= 0.0 {
        return source_pixel;
    }
    // The bounded distance transform stores no site away from the stroke.
    // Most of the full-frame pixels can reuse the source without subpixel work.
    let nearest_site = decode_site(textureLoad(effect_input, position, 0));
    if nearest_site.x < 0 {
        return source_pixel;
    }

    // Estimate material opacity separately from subpixel edge coverage. This
    // keeps the silhouette and stroke invariant when source opacity changes.
    let opacity = max(local_opacity(position, dimensions), local_opacity(nearest_site, dimensions));
    if opacity <= 0.0 {
        return source_pixel;
    }
    let stroke = premul(properties.color, 1.0);
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
            let distance_pixels = distance_at(sample_position, position, dimensions, opacity);
            let coverage = clamp(radius + 1.0 - distance_pixels, 0.0, 1.0);
            let silhouette = clamp(source.a / opacity, 0.0, 1.0);
            let outside = coverage * (1.0 - silhouette);
            let inside = coverage * silhouette;

            // Inside and outside cover disjoint parts of the pixel. Applying
            // source-over to these coverage-weighted colors again would count
            // edge coverage twice, creating seams and dimming translucent fills.
            if properties.position == 0u {
                result += source + stroke * outside;
            } else if properties.position == 2u {
                result += source * (1.0 - stroke.a * coverage) + stroke * inside;
            } else {
                result += source * (1.0 - stroke.a * coverage) + stroke * (inside + outside);
            }
        }
    }
    return result / 4.0;
}
