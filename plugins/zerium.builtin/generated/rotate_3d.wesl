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
    v2: f32,
}

struct ZeriumPropsTuple2 {
    v0: f32,
    v1: f32,
}

struct ZeriumProps {
    rotation: ZeriumPropsTuple0,
    perspective: f32,
    center: ZeriumPropsTuple2,
}

fn props() -> ZeriumProps {
    let raw = pass_props();
    return ZeriumProps(
        ZeriumPropsTuple0(read_f32(raw, 0u), read_f32(raw, 0u + 4u), read_f32(raw, 0u + 8u)),
        read_f32(raw, 12u),
        ZeriumPropsTuple2(read_f32(raw, 16u), read_f32(raw, 16u + 4u)),
    );
}
