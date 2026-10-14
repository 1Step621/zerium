import package::generated::host::_effect::{ZeriumRawProps, effect_props, read_u32, read_i32, read_f32, read_bool};

struct ZeriumProps {
    start_color: vec4<f32>,
    end_color: vec4<f32>,
    angle: f32,
    grain_size: f32,
    width: f32,
    offset: f32,
    phase: f32,
    seed: u32,
    amount: f32,
};

fn props() -> ZeriumProps {
    let raw = effect_props();
    return ZeriumProps(vec4(read_f32(raw, 0u), read_f32(raw, 0u + 4u), read_f32(raw, 0u + 8u), read_f32(raw, 0u + 12u)), vec4(read_f32(raw, 16u), read_f32(raw, 16u + 4u), read_f32(raw, 16u + 8u), read_f32(raw, 16u + 12u)), read_f32(raw, 32u), read_f32(raw, 36u), read_f32(raw, 40u), read_f32(raw, 44u), read_f32(raw, 48u), read_u32(raw, 52u), read_f32(raw, 56u));
}
