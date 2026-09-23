import package::generated::compute::{ZeriumRawProps, effect_props, read_u32, read_i32, read_f32, read_bool};

struct ZeriumProps {
    _raw: ZeriumRawProps,
    start_color: vec4<f32>,
    end_color: vec4<f32>,
    angle: f32,
};

fn props() -> ZeriumProps {
    let raw = effect_props();
    return ZeriumProps(raw, vec4(read_f32(raw, 0u), read_f32(raw, 0u + 4u), read_f32(raw, 0u + 8u), read_f32(raw, 0u + 12u)), vec4(read_f32(raw, 16u), read_f32(raw, 16u + 4u), read_f32(raw, 16u + 8u), read_f32(raw, 16u + 12u)), read_f32(raw, 32u));
}
