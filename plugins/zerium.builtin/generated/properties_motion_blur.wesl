import package::generated::temporal::{ZeriumTemporal, temporal_sample, temporal_accumulation, temporal_sampler, temporal_state, property_words, effect_props, info, context, ZeriumVertex, vertex, ZeriumRawProps, read_word, read_u32, read_i32, read_f32, read_bool};

struct ZeriumProps {
    _raw: ZeriumRawProps,
    samples: u32,
    shutter_angle: f32,
    shutter_phase: f32,
};

fn props() -> ZeriumProps {
    let raw = effect_props();
    return ZeriumProps(raw, read_u32(raw, 0u), read_f32(raw, 4u), read_f32(raw, 8u));
}
