import package::generated::host::_effect::{ZeriumRawProps, effect_props, read_u32, read_i32, read_f32, read_bool};

struct ZeriumProps {
    direction: u32,
    angle: f32,
    amount: f32,
};

fn props() -> ZeriumProps {
    let raw = effect_props();
    return ZeriumProps(read_u32(raw, 0u), read_f32(raw, 4u), read_f32(raw, 8u));
}
