@vertex
fn vertex_main(@builtin(vertex_index) vertex_index: u32) -> ZeriumEffectVertexOutput {
    return zerium_effect_vertex(vertex_index);
}

@fragment
fn fragment_main(input: ZeriumEffectVertexOutput) -> @location(0) vec4<f32> {
    let params = zerium_load_parameters();
    let source = textureSample(zerium_effect_input, zerium_effect_sampler, input.uv);
    let alpha = source.a;
    var color = zerium_straight_rgb(source);

    color += vec3(params.brightness);
    color = (color - vec3(0.5)) * params.contrast + vec3(0.5);
    let luminance = dot(color, vec3(0.2126, 0.7152, 0.0722));
    color = mix(vec3(luminance), color, params.saturation);
    color += vec3(params.temperature * 0.12, 0.0, -params.temperature * 0.12);
    color += vec3(params.tint * 0.06, -params.tint * 0.06, params.tint * 0.06);
    color = pow(max(color, vec3(0.0)), vec3(1.0 / max(params.gamma, 0.01)));

    return vec4(max(color, vec3(0.0)) * alpha, alpha);
}
