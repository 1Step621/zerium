@vertex
fn vertex_main(@builtin(vertex_index) vertex_index: u32) -> ZeriumEffectVertexOutput {
    return zerium_effect_vertex(vertex_index);
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
    return textureSampleLevel(zerium_effect_source, zerium_effect_sampler, uv, 0.0);
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
            let site = decode_site(textureLoad(zerium_effect_input, candidate_position, 0));
            if site.x < 0 {
                continue;
            }
            let site_alpha = textureLoad(zerium_effect_source, site, 0).a;
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
fn fragment_main(input: ZeriumEffectVertexOutput) -> @location(0) vec4<f32> {
    let params = zerium_load_parameters();
    let dimensions = vec2<i32>(textureDimensions(zerium_effect_input));
    let position = clamp(
        vec2<i32>(input.uv * vec2<f32>(dimensions)),
        vec2(0),
        dimensions - vec2(1),
    );
    if params.width <= 0.0 || params.intensity <= 0.0 {
        return textureLoad(zerium_effect_source, position, 0);
    }

    let composition_scale = zerium_render_context().composition_scale;
    let radius = max(params.width, 0.0001);
    let solid_radius = radius * clamp(params.spread, 0.0, 1.0);
    let sigma = max((radius - solid_radius) / 3.0, 0.5);

    var result = vec4(0.0);
    for (var sample_y = 0u; sample_y < 4u; sample_y += 1u) {
        for (var sample_x = 0u; sample_x < 4u; sample_x += 1u) {
            let offset = (vec2(f32(sample_x), f32(sample_y)) + vec2(0.5)) / 4.0
                - vec2(0.5);
            let sample_position = vec2<f32>(position) + offset;
            let source = source_sample(sample_position, dimensions);
            let distance = distance_at(sample_position, position, dimensions)
                / max(composition_scale, 0.0001);
            let soft_distance = max(distance - solid_radius, 0.0);
            let falloff = exp(
                -(soft_distance * soft_distance) / (2.0 * sigma * sigma),
            );
            let glow_alpha = clamp(
                falloff * params.intensity * params.color.a
                    * (1.0 - source.a),
                0.0,
                1.0,
            );
            let glow = vec4(
                zerium_srgb_to_scene(params.color.rgb) * glow_alpha,
                glow_alpha,
            );
            result += zerium_composite_over(source, glow);
        }
    }
    return result / 16.0;
}
