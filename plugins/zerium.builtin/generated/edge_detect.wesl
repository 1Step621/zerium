import package::generated::_internal::properties::{
    ZeriumRawProps,
    read_bool,
    read_f32,
    read_i32,
    read_u32
};
import package::generated::_internal::effect_state::pass_props;

struct ZeriumProps {
    width: f32,
    strength: f32,
    color: vec4<f32>,
    background: vec4<f32>,
}

fn props() -> ZeriumProps {
    let raw = pass_props();
    return ZeriumProps(
        read_f32(raw, 0u),
        read_f32(raw, 4u),
        vec4(read_f32(raw, 8u), read_f32(raw, 8u + 4u), read_f32(raw, 8u + 8u), read_f32(raw, 8u + 12u)),
        vec4(read_f32(raw, 24u), read_f32(raw, 24u + 4u), read_f32(raw, 24u + 8u), read_f32(raw, 24u + 12u)),
    );
}
