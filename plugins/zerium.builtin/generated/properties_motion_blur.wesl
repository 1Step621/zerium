import package::generated::temporal::{ZeriumRawProps, effect_props, read_u32, read_i32, read_f32, read_bool};

struct ZeriumProps {
    _raw: ZeriumRawProps,
    samples: u32,
    start_offset: f32,
    end_offset: f32,
};

fn props() -> ZeriumProps {
    let raw = effect_props();
    return ZeriumProps(raw, read_u32(raw, 0u), read_f32(raw, 4u), read_f32(raw, 8u));
}
