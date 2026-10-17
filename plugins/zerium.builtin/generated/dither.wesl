import package::generated::_internal::properties::{
    ZeriumRawProps,
    read_bool,
    read_f32,
    read_i32,
    read_u32
};
import package::generated::_internal::effect_state::pass_props;

struct ZeriumProps {
    pattern: u32,
    levels: u32,
    cell_size: f32,
    grayscale: bool,
    seed: u32,
}

fn props() -> ZeriumProps {
    let raw = pass_props();
    return ZeriumProps(
        read_u32(raw, 0u),
        read_u32(raw, 4u),
        read_f32(raw, 8u),
        read_bool(raw, 12u),
        read_u32(raw, 16u),
    );
}
