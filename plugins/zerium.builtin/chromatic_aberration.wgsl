@vertex
fn vertex_main(@builtin(vertex_index) vertex_index: u32) -> ZeriumEffectVertexOutput {
    return zerium_effect_vertex(vertex_index);
}

@fragment
fn fragment_main(input: ZeriumEffectVertexOutput) -> @location(0) vec4<f32> {
    let params = zerium_load_parameters();
    let angle = params.angle * ZERIUM_DEGREES_TO_RADIANS;
    let offset = vec2(cos(angle), sin(angle)) * params.amount
        / zerium_render_context().composition_size;
    let red_sample = textureSample(zerium_effect_input, zerium_effect_sampler, input.uv + offset);
    let center_sample = textureSample(zerium_effect_input, zerium_effect_sampler, input.uv);
    let blue_sample = textureSample(zerium_effect_input, zerium_effect_sampler, input.uv - offset);
    let alpha = max(red_sample.a, max(center_sample.a, blue_sample.a));
    let color = vec3(
        zerium_straight_rgb(red_sample).r,
        zerium_straight_rgb(center_sample).g,
        zerium_straight_rgb(blue_sample).b,
    );
    return vec4(color * alpha, alpha);
}
