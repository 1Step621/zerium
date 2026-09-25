import package::generated::compute::{ZeriumRawProps, effect_props, read_u32, read_i32, read_f32, read_bool};

struct ZeriumProps {
    _raw: ZeriumRawProps,
    threshold: f32,
    vertical: bool,
    descending: bool,
};

fn props() -> ZeriumProps {
    let raw = effect_props();
    return ZeriumProps(raw, read_f32(raw, 0u), read_bool(raw, 4u), read_bool(raw, 8u));
}
