import package::generated::_internal::properties::{
    ZeriumRawProps,
    read_bool,
    read_f32,
    read_i32,
    read_u32
};
import package::generated::_internal::effect_state::pass_props;

struct ZeriumProps {
    threshold: f32,
    dark_color: vec4<f32>,
    light_color: vec4<f32>,
    quantize_alpha: bool,
    alpha_threshold: f32,
}

fn props() -> ZeriumProps {
    let raw = pass_props();
    return ZeriumProps(
        read_f32(raw, 0u),
        vec4(read_f32(raw, 4u), read_f32(raw, 4u + 4u), read_f32(raw, 4u + 8u), read_f32(raw, 4u + 12u)),
        vec4(read_f32(raw, 20u), read_f32(raw, 20u + 4u), read_f32(raw, 20u + 8u), read_f32(raw, 20u + 12u)),
        read_bool(raw, 36u),
        read_f32(raw, 40u),
    );
}
