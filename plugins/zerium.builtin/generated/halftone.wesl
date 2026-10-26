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
    size: f32,
    angle: f32,
    ink: vec4<f32>,
    paper: vec4<f32>,
    amount: f32,
}

fn props() -> ZeriumProps {
    let raw = pass_props();
    return ZeriumProps(
        read_u32(raw, 0u),
        read_f32(raw, 4u),
        read_f32(raw, 8u),
        vec4(read_f32(raw, 12u), read_f32(raw, 12u + 4u), read_f32(raw, 12u + 8u), read_f32(raw, 12u + 12u)),
        vec4(read_f32(raw, 28u), read_f32(raw, 28u + 4u), read_f32(raw, 28u + 8u), read_f32(raw, 28u + 12u)),
        read_f32(raw, 44u),
    );
}
