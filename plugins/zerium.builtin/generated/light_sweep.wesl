import package::generated::_internal::properties::{
    ZeriumRawProps,
    read_bool,
    read_f32,
    read_i32,
    read_u32
};
import package::generated::_internal::effect_state::pass_props;

struct ZeriumPropsTuple0 {
    v0: f32,
    v1: f32,
}

struct ZeriumProps {
    position: ZeriumPropsTuple0,
    angle: f32,
    width: f32,
    softness: f32,
    intensity: f32,
    color: vec4<f32>,
}

fn props() -> ZeriumProps {
    let raw = pass_props();
    return ZeriumProps(
        ZeriumPropsTuple0(read_f32(raw, 0u), read_f32(raw, 0u + 4u)),
        read_f32(raw, 8u),
        read_f32(raw, 12u),
        read_f32(raw, 16u),
        read_f32(raw, 20u),
        vec4(read_f32(raw, 24u), read_f32(raw, 24u + 4u), read_f32(raw, 24u + 8u), read_f32(raw, 24u + 12u)),
    );
}
