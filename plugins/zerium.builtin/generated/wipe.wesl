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
    progress: f32,
    angle: f32,
    softness: f32,
    invert: bool,
}

fn props() -> ZeriumProps {
    let raw = pass_props();
    return ZeriumProps(
        read_u32(raw, 0u),
        read_f32(raw, 4u),
        read_f32(raw, 8u),
        read_f32(raw, 12u),
        read_bool(raw, 16u),
    );
}
