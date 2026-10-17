import package::generated::_internal::properties::{
    ZeriumRawProps,
    read_bool,
    read_f32,
    read_i32,
    read_u32
};
import package::generated::_internal::effect_state::pass_props;

struct ZeriumProps {
    start_offset: u32,
    end_offset: u32,
    hide_original: bool,
    invert_mask: bool,
}

fn props() -> ZeriumProps {
    let raw = pass_props();
    return ZeriumProps(
        read_u32(raw, 0u),
        read_u32(raw, 4u),
        read_bool(raw, 8u),
        read_bool(raw, 12u),
    );
}
