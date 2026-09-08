@vertex
fn vertex_main(@builtin(vertex_index) vertex_index: u32) -> ZeriumEffectVertexOutput {
    return zerium_effect_vertex(vertex_index);
}

@fragment
fn fragment_main(input: ZeriumEffectVertexOutput) -> @location(0) vec4<f32> {
    let params = zerium_load_parameters();
    let dimensions = zerium_render_context().output_size;
    let block_size = max(round(params.block_size * zerium_render_context().composition_scale), 1.0);
    let block_center = (floor(input.uv * dimensions / block_size) + vec2(0.5)) * block_size;
    let half_texel = vec2(0.5) / dimensions;
    let sample_uv = clamp(block_center / dimensions, half_texel, vec2(1.0) - half_texel);
    return textureSample(zerium_effect_input, zerium_effect_sampler, sample_uv);
}
