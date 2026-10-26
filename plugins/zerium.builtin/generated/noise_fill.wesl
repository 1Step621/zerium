import package::generated::_internal::properties::{
    ZeriumRawProps,
    read_bool,
    read_f32,
    read_i32,
    read_u32
};
import package::generated::_internal::effect_state::pass_props;

struct ZeriumProps {
    mode: u32,
    scale: f32,
    detail: u32,
    phase: f32,
    seed: u32,
    contrast: f32,
    dark: vec4<f32>,
    light: vec4<f32>,
    amount: f32,
}

fn props() -> ZeriumProps {
    let raw = pass_props();
    return ZeriumProps(
        read_u32(raw, 0u),
        read_f32(raw, 4u),
        read_u32(raw, 8u),
        read_f32(raw, 12u),
        read_u32(raw, 16u),
        read_f32(raw, 20u),
        vec4(read_f32(raw, 24u), read_f32(raw, 24u + 4u), read_f32(raw, 24u + 8u), read_f32(raw, 24u + 12u)),
        vec4(read_f32(raw, 40u), read_f32(raw, 40u + 4u), read_f32(raw, 40u + 8u), read_f32(raw, 40u + 12u)),
        read_f32(raw, 56u),
    );
}
