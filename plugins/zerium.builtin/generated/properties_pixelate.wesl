import package::generated::effect::{ZeriumRawProps, effect_props, read_u32, read_i32, read_f32, read_bool};

struct ZeriumProps {
    _raw: ZeriumRawProps,
    block_size: f32,
};

fn props() -> ZeriumProps {
    let raw = effect_props();
    return ZeriumProps(raw, read_f32(raw, 0u));
}
