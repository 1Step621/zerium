import package::generated::host::_effect::{ZeriumRawProps, effect_props, read_u32, read_i32, read_f32, read_bool};

struct ZeriumProps {
    threshold: f32,
    dark_color: vec4<f32>,
    light_color: vec4<f32>,
    quantize_alpha: bool,
    alpha_threshold: f32,
};

fn props() -> ZeriumProps {
    let raw = effect_props();
    return ZeriumProps(read_f32(raw, 0u), vec4(read_f32(raw, 4u), read_f32(raw, 4u + 4u), read_f32(raw, 4u + 8u), read_f32(raw, 4u + 12u)), vec4(read_f32(raw, 20u), read_f32(raw, 20u + 4u), read_f32(raw, 20u + 8u), read_f32(raw, 20u + 12u)), read_bool(raw, 36u), read_f32(raw, 40u));
}
