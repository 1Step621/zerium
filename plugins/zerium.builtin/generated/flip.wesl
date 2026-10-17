import package::generated::_internal::properties::{
    ZeriumRawProps,
    read_bool,
    read_f32,
    read_i32,
    read_u32
};
import package::generated::_internal::effect_state::pass_props;

struct ZeriumProps {
    horizontal: bool,
    vertical: bool,
}

fn props() -> ZeriumProps {
    let raw = pass_props();
    return ZeriumProps(
        read_bool(raw, 0u),
        read_bool(raw, 4u),
    );
}
