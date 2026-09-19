import package::generated::compute::{ZeriumCompute, effect_input, effect_sampler, compute_info, property_words, effect_output, effect_source, effect_props, context, store, ZeriumRawProps, read_word, read_u32, read_i32, read_f32, read_bool};

struct ZeriumProps {
    _raw: ZeriumRawProps,
    block_size: u32,
    threshold: f32,
    vertical: bool,
    descending: bool,
};

fn props() -> ZeriumProps {
    let raw = effect_props();
    return ZeriumProps(raw, read_u32(raw, 0u), read_f32(raw, 4u), read_bool(raw, 8u), read_bool(raw, 12u));
}
