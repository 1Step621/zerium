import package::generated::effect::{ZeriumRawProps, effect_props, read_u32, read_i32, read_f32, read_bool};

struct ZeriumProps {
    _raw: ZeriumRawProps,
    start_offset: u32,
    end_offset: u32,
    hide_original: bool,
    invert_mask: bool,
};

fn props() -> ZeriumProps {
    let raw = effect_props();
    return ZeriumProps(raw, read_u32(raw, 0u), read_u32(raw, 4u), read_bool(raw, 8u), read_bool(raw, 12u));
}
