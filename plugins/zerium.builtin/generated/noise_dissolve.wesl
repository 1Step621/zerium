import package::generated::_internal::properties::{
    ZeriumRawProps,
    read_bool,
    read_f32,
    read_i32,
    read_u32
};
import package::generated::_internal::effect_state::pass_props;

struct ZeriumProps {
    progress: f32,
    scale: f32,
    detail: u32,
    phase: f32,
    seed: u32,
    softness: f32,
}

fn props() -> ZeriumProps {
    let raw = pass_props();
    return ZeriumProps(
        read_f32(raw, 0u),
        read_f32(raw, 4u),
        read_u32(raw, 8u),
        read_f32(raw, 12u),
        read_u32(raw, 16u),
        read_f32(raw, 20u),
    );
}
