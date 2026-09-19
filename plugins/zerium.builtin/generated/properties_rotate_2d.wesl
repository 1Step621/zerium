import package::generated::effect::{ZeriumEffect, effect_input, effect_sampler, effect_instance, property_words, effect_source, effect_props, context, ZeriumVertex, vertex, uv_to_position, position_to_uv, uv_inside, sample_or_transparent, straight, rotate, over, premul, ZeriumRawProps, read_word, read_u32, read_i32, read_f32, read_bool};

struct ZeriumPropsTuple1 {
    v0: f32,
    v1: f32,
};

struct ZeriumProps {
    _raw: ZeriumRawProps,
    angle: f32,
    center: ZeriumPropsTuple1,
};

fn props() -> ZeriumProps {
    let raw = effect_props();
    return ZeriumProps(raw, read_f32(raw, 0u), ZeriumPropsTuple1(read_f32(raw, 4u), read_f32(raw, 4u + 4u)));
}
