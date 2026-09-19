import package::generated::effect::{ZeriumEffect, effect_input, effect_sampler, effect_instance, property_words, effect_source, effect_props, context, ZeriumVertex, vertex, uv_to_position, position_to_uv, uv_inside, sample_or_transparent, straight, rotate, over, premul, ZeriumRawProps, read_word, read_u32, read_i32, read_f32, read_bool};

struct ZeriumPropsTuple0 {
    v0: f32,
    v1: f32,
    v2: f32,
};

struct ZeriumPropsTuple2 {
    v0: f32,
    v1: f32,
};

struct ZeriumProps {
    _raw: ZeriumRawProps,
    rotation: ZeriumPropsTuple0,
    perspective: f32,
    center: ZeriumPropsTuple2,
};

fn props() -> ZeriumProps {
    let raw = effect_props();
    return ZeriumProps(raw, ZeriumPropsTuple0(read_f32(raw, 0u), read_f32(raw, 0u + 4u), read_f32(raw, 0u + 8u)), read_f32(raw, 12u), ZeriumPropsTuple2(read_f32(raw, 16u), read_f32(raw, 16u + 4u)));
}
