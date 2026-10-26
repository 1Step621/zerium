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

struct ZeriumPropsTuple1 {
    v0: f32,
    v1: f32,
}

struct ZeriumPropsTuple2 {
    v0: f32,
    v1: f32,
}

struct ZeriumPropsTuple3 {
    v0: f32,
    v1: f32,
}

struct ZeriumProps {
    top_left: ZeriumPropsTuple0,
    top_right: ZeriumPropsTuple1,
    bottom_right: ZeriumPropsTuple2,
    bottom_left: ZeriumPropsTuple3,
}

fn props() -> ZeriumProps {
    let raw = pass_props();
    return ZeriumProps(
        ZeriumPropsTuple0(read_f32(raw, 0u), read_f32(raw, 0u + 4u)),
        ZeriumPropsTuple1(read_f32(raw, 8u), read_f32(raw, 8u + 4u)),
        ZeriumPropsTuple2(read_f32(raw, 16u), read_f32(raw, 16u + 4u)),
        ZeriumPropsTuple3(read_f32(raw, 24u), read_f32(raw, 24u + 4u)),
    );
}
