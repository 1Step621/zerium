@vertex
fn vertex_main(@builtin(vertex_index) vertex_index: u32) -> ZeriumEffectVertexOutput {
    return zerium_effect_vertex(vertex_index);
}

@fragment
fn fragment_main(input: ZeriumEffectVertexOutput) -> @location(0) vec4<f32> {
    let params = zerium_load_parameters();
    let center = vec2(params.center.v0, params.center.v1);
    let output_position = zerium_effect_uv_to_composition_position(input.uv);
    let relative_position = output_position - center;
    let angle = -params.angle * ZERIUM_DEGREES_TO_RADIANS;
    let source_position = zerium_rotate_2d(relative_position, angle) + center;
    let source_uv = zerium_composition_position_to_effect_uv(source_position);
    return zerium_sample_effect_input_or_transparent(source_uv);
}
