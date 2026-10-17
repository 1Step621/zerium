import package::generated::_internal::properties::{
    ZeriumRawProps,
    read_bool,
    read_f32,
    read_i32,
    read_u32
};
import package::generated::_internal::effect_state::pass_props;

struct ZeriumPropsTuple1 {
    v0: f32,
    v1: f32,
}

struct ZeriumProps {
    angle: f32,
    center: ZeriumPropsTuple1,
}

fn props() -> ZeriumProps {
    let raw = pass_props();
    return ZeriumProps(
        read_f32(raw, 0u),
        ZeriumPropsTuple1(read_f32(raw, 4u), read_f32(raw, 4u + 4u)),
    );
}
