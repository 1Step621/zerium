import package::generated::_internal::properties::{
    ZeriumRawProps,
    read_bool,
    read_f32,
    read_i32,
    read_u32
};
import package::generated::_internal::effect_state::pass_props;

struct ZeriumPropsTuple3 {
    v0: f32,
    v1: f32,
}

struct ZeriumProps {
    start_offset: u32,
    end_offset: u32,
    hide_original: bool,
    displacement: ZeriumPropsTuple3,
    fit_map: bool,
    channel_x: u32,
    channel_y: u32,
}

fn props() -> ZeriumProps {
    let raw = pass_props();
    return ZeriumProps(
        read_u32(raw, 0u),
        read_u32(raw, 4u),
        read_bool(raw, 8u),
        ZeriumPropsTuple3(read_f32(raw, 12u), read_f32(raw, 12u + 4u)),
        read_bool(raw, 20u),
        read_u32(raw, 24u),
        read_u32(raw, 28u),
    );
}
