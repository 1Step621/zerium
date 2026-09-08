@vertex
fn vertex_main(@builtin(vertex_index) vertex_index: u32) -> ZeriumEffectVertexOutput {
    return zerium_effect_vertex(vertex_index);
}

@fragment
fn fragment_main(input: ZeriumEffectVertexOutput) -> @location(0) vec4<f32> {
    let params = zerium_load_parameters();
    let center = vec2(params.center.v0, params.center.v1);
    let output_position = zerium_effect_uv_to_composition_position(input.uv) - center;

    let radians = vec3(params.rotation.v0, params.rotation.v1, params.rotation.v2)
        * ZERIUM_DEGREES_TO_RADIANS;
    let sine = sin(radians);
    let cosine = cos(radians);

    // First two columns of Rz * Ry * Rx. The source is a flat z=0 plane.
    let r00 = cosine.z * cosine.y;
    let r10 = sine.z * cosine.y;
    let r20 = -sine.y;
    let r01 = cosine.z * sine.y * sine.x - sine.z * cosine.x;
    let r11 = sine.z * sine.y * sine.x + cosine.z * cosine.x;
    let r21 = cosine.y * sine.x;

    let focal_length = max(params.perspective, 1.0);
    let a = output_position.x * r20 - focal_length * r00;
    let b = output_position.x * r21 - focal_length * r01;
    let c = output_position.y * r20 - focal_length * r10;
    let d = output_position.y * r21 - focal_length * r11;
    let determinant = a * d - b * c;
    if abs(determinant) < 0.00001 {
        return vec4(0.0);
    }

    let right_hand_side = -output_position * focal_length;
    let source_relative = vec2(
        (right_hand_side.x * d - b * right_hand_side.y) / determinant,
        (a * right_hand_side.y - right_hand_side.x * c) / determinant,
    );
    let depth = focal_length + r20 * source_relative.x + r21 * source_relative.y;
    if depth <= 0.00001 {
        return vec4(0.0);
    }

    let source_position = source_relative + center;
    let source_uv = zerium_composition_position_to_effect_uv(source_position);
    return zerium_sample_effect_input_or_transparent(source_uv);
}
