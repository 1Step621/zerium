@vertex
fn vertex_main(@builtin(vertex_index) vertex_index: u32) -> ZeriumEffectVertexOutput {
    return zerium_effect_vertex(vertex_index);
}

@fragment
fn fragment_main(input: ZeriumEffectVertexOutput) -> @location(0) vec4<f32> {
    let params = zerium_load_parameters();
    let source = textureSample(zerium_effect_input, zerium_effect_sampler, input.uv);
    let dimensions = zerium_render_context().output_size;
    let point = input.uv * dimensions;
    let center = dimensions * 0.5;
    let angle = params.angle * ZERIUM_DEGREES_TO_RADIANS;
    let direction = vec2(cos(angle), sin(angle));
    let half_span = max(dot(abs(direction), dimensions) * 0.5, 0.00001);
    let amount = clamp(dot(point - center, direction) / (half_span * 2.0) + 0.5, 0.0, 1.0);
    let color = mix(params.start_color, params.end_color, amount);
    let alpha = source.a * color.a;
    return vec4(zerium_srgb_to_scene(color.rgb) * alpha, alpha);
}
