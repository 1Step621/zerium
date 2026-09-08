@vertex
fn vertex_main(@builtin(vertex_index) vertex_index: u32) -> ZeriumEffectVertexOutput {
    return zerium_effect_vertex(vertex_index);
}

@fragment
fn fragment_main(input: ZeriumEffectVertexOutput) -> @location(0) vec4<f32> {
    let params = zerium_load_parameters();
    let offset = vec2(params.offset.v0, params.offset.v1);
    let source_uv = input.uv - offset / zerium_render_context().composition_size;
    return zerium_sample_effect_input_or_transparent(source_uv);
}
