@vertex
fn vertex_main(@builtin(vertex_index) vertex_index: u32) -> ZeriumEffectVertexOutput {
    return zerium_effect_vertex(vertex_index);
}

@fragment
fn fragment_main(input: ZeriumEffectVertexOutput) -> @location(0) vec4<f32> {
    let params = zerium_load_parameters();
    let amount = clamp(params.opacity / 100.0, 0.0, 1.0);
    return textureSample(zerium_effect_input, zerium_effect_sampler, input.uv) * amount;
}
