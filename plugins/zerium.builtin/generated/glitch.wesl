import package::generated::_internal::properties::{
    ZeriumRawProps,
    read_bool,
    read_f32,
    read_i32,
    read_u32
};
import package::generated::_internal::effect_state::pass_props;

struct ZeriumProps {
    displacement: f32,
    block_height: f32,
    density: f32,
    separation: f32,
    phase: f32,
    seed: u32,
}

fn props() -> ZeriumProps {
    let raw = pass_props();
    return ZeriumProps(
        read_f32(raw, 0u),
        read_f32(raw, 4u),
        read_f32(raw, 8u),
        read_f32(raw, 12u),
        read_f32(raw, 16u),
        read_u32(raw, 20u),
    );
}
