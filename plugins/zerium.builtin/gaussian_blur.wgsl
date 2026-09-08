const BLUR_OFFSETS: array<f32, 9> = array(
    -1.0, -0.75, -0.5, -0.25, 0.0, 0.25, 0.5, 0.75, 1.0,
);
const BLUR_WEIGHTS: array<f32, 9> = array(
    0.0276306, 0.0662822, 0.1238315, 0.1801738, 0.2041638,
    0.1801738, 0.1238315, 0.0662822, 0.0276306,
);

@compute @workgroup_size(8, 8, 1)
fn compute_main(@builtin(global_invocation_id) global_id: vec3<u32>) {
    let output_size = vec2<u32>(zerium_render_context().output_size);
    if global_id.x >= output_size.x || global_id.y >= output_size.y {
        return;
    }

    let params = zerium_load_parameters();
    let dimensions = vec2<f32>(output_size);
    let uv = (vec2<f32>(global_id.xy) + vec2(0.5)) / dimensions;
    let radius = clamp(params.radius * zerium_render_context().composition_scale, 0.0, 256.0);
    if radius < 0.5 {
        zerium_store_output(
            global_id.xy,
            textureSampleLevel(zerium_effect_input, zerium_effect_sampler, uv, 0.0),
        );
        return;
    }

    let direction = vec2(params.direction.v0, params.direction.v1);
    var color = vec4(0.0);
    for (var tap = 0u; tap < 9u; tap += 1u) {
        let sample_uv = uv + direction * BLUR_OFFSETS[tap] * radius / dimensions;
        color += textureSampleLevel(
            zerium_effect_input,
            zerium_effect_sampler,
            sample_uv,
            0.0,
        ) * BLUR_WEIGHTS[tap];
    }
    zerium_store_output(global_id.xy, color);
}
