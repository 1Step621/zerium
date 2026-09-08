struct ZeriumRawParams {
    offset: u32,
    size: u32,
};

fn zerium_raw_word(params: ZeriumRawParams, word_index: u32) -> u32 {
    if word_index * 4u >= params.size {
        return 0u;
    }
    return zerium_parameter_words[params.offset + word_index];
}

fn zerium_raw_u32(params: ZeriumRawParams, byte_offset: u32) -> u32 {
    if byte_offset + 4u > params.size {
        return 0u;
    }
    return zerium_raw_word(params, byte_offset / 4u);
}

fn zerium_raw_i32(params: ZeriumRawParams, byte_offset: u32) -> i32 {
    return bitcast<i32>(zerium_raw_u32(params, byte_offset));
}

fn zerium_raw_f32(params: ZeriumRawParams, byte_offset: u32) -> f32 {
    return bitcast<f32>(zerium_raw_u32(params, byte_offset));
}

fn zerium_raw_bool(params: ZeriumRawParams, byte_offset: u32) -> bool {
    return zerium_raw_u32(params, byte_offset) != 0u;
}

struct ZeriumRenderContext {
    output_size: vec2<f32>,
    composition_size: vec2<f32>,
    composition_scale: f32,
};

fn zerium_make_render_context(
    output_size: vec2<f32>,
    composition_size: vec2<f32>,
) -> ZeriumRenderContext {
    let scale = output_size / composition_size;
    return ZeriumRenderContext(output_size, composition_size, min(scale.x, scale.y));
}

fn zerium_srgb_to_scene_channel(value: f32) -> f32 {
    if value <= 0.04045 {
        return value / 12.92;
    }
    return pow((value + 0.055) / 1.055, 2.4);
}

fn zerium_srgb_to_scene(color: vec3<f32>) -> vec3<f32> {
    return vec3(
        zerium_srgb_to_scene_channel(color.r),
        zerium_srgb_to_scene_channel(color.g),
        zerium_srgb_to_scene_channel(color.b),
    );
}

fn zerium_scene_premultiplied_color(color: vec4<f32>) -> vec4<f32> {
    let alpha = clamp(color.a, 0.0, 1.0);
    return vec4(zerium_srgb_to_scene(color.rgb) * alpha, alpha);
}

const ZERIUM_DEGREES_TO_RADIANS: f32 = 0.017453292519943295;
