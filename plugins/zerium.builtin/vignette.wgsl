@vertex
fn vertex_main(@builtin(vertex_index) vertex_index: u32) -> ZeriumEffectVertexOutput {
    return zerium_effect_vertex(vertex_index);
}

@fragment
fn fragment_main(input: ZeriumEffectVertexOutput) -> @location(0) vec4<f32> {
    let params = zerium_load_parameters();
    let dimensions = zerium_render_context().output_size;
    let aspect = dimensions.x / max(dimensions.y, 1.0);
    var centered = input.uv * 2.0 - vec2(1.0);
    centered.x *= aspect * max(params.roundness, 0.01);
    let corner_distance = length(vec2(aspect * max(params.roundness, 0.01), 1.0));
    let distance = length(centered) / max(corner_distance, 0.00001);
    let vignette = smoothstep(
        max(1.0 - params.softness, 0.0),
        1.0,
        distance,
    ) * clamp(params.amount, 0.0, 1.0);
    let source = textureSample(zerium_effect_input, zerium_effect_sampler, input.uv);
    return vec4(source.rgb * (1.0 - vignette), source.a);
}
