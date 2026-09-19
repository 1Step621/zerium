struct ZeriumContext {
    output_size: vec2<f32>,
    composition_size: vec2<f32>,
    composition_scale: f32,
};

fn make_context(
    output_size: vec2<f32>,
    composition_size: vec2<f32>,
) -> ZeriumContext {
    let scale = output_size / composition_size;
    return ZeriumContext(output_size, composition_size, min(scale.x, scale.y));
}

fn srgb_channel(value: f32) -> f32 {
    if value <= 0.04045 {
        return value / 12.92;
    }
    return pow((value + 0.055) / 1.055, 2.4);
}

fn srgb(color: vec3<f32>) -> vec3<f32> {
    return vec3(
        srgb_channel(color.r),
        srgb_channel(color.g),
        srgb_channel(color.b),
    );
}

fn scene_color(color: vec4<f32>) -> vec4<f32> {
    let alpha = clamp(color.a, 0.0, 1.0);
    return vec4(srgb(color.rgb) * alpha, alpha);
}

const DEG2RAD: f32 = 0.017453292519943295;
