import package::generated::host::_props::{ZeriumRawProps, effect_props, read_u32, read_i32, read_f32, read_bool};

struct ZeriumProps {
    brightness: f32,
    contrast: f32,
    saturation: f32,
    temperature: f32,
    tint: f32,
    gamma: f32,
};

fn props() -> ZeriumProps {
    let raw = effect_props();
    return ZeriumProps(read_f32(raw, 0u), read_f32(raw, 4u), read_f32(raw, 8u), read_f32(raw, 12u), read_f32(raw, 16u), read_f32(raw, 20u));
}
