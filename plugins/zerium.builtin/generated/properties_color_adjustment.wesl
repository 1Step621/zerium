import package::generated::effect::{ZeriumRawProps, effect_props, read_u32, read_i32, read_f32, read_bool};

struct ZeriumProps {
    _raw: ZeriumRawProps,
    brightness: f32,
    contrast: f32,
    saturation: f32,
    temperature: f32,
    tint: f32,
    gamma: f32,
};

fn props() -> ZeriumProps {
    let raw = effect_props();
    return ZeriumProps(raw, read_f32(raw, 0u), read_f32(raw, 4u), read_f32(raw, 8u), read_f32(raw, 12u), read_f32(raw, 16u), read_f32(raw, 20u));
}
