import package::generated::host::_effect::{ZeriumRawProps, effect_props, read_u32, read_i32, read_f32, read_bool};

struct ZeriumProps {
    amount: f32,
    scale: f32,
    octaves: u32,
    phase: f32,
    seed: u32,
};

fn props() -> ZeriumProps {
    let raw = effect_props();
    return ZeriumProps(read_f32(raw, 0u), read_f32(raw, 4u), read_u32(raw, 8u), read_f32(raw, 12u), read_u32(raw, 16u));
}
