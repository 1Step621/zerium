import package::generated::_internal::properties::{
    ZeriumRawProps,
    read_bool,
    read_f32,
    read_i32,
    read_u32
};
import package::generated::_internal::item::item_props;

struct ZeriumPropsTuple0 {
    v0: f32,
    v1: f32,
}

struct ZeriumPropsTuple1 {
    v0: u32,
    v1: u32,
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
    position: ZeriumPropsTuple0,
    origin: ZeriumPropsTuple1,
    size: ZeriumPropsTuple2,
    tail: ZeriumPropsTuple3,
    tail_width: f32,
    corner_radius: f32,
    fill_enabled: bool,
    color: vec4<f32>,
    outline_width: f32,
    outline_position: u32,
    outline_color: vec4<f32>,
}

fn props(instance_index: u32) -> ZeriumProps {
    let raw = item_props(instance_index);
    return ZeriumProps(
        ZeriumPropsTuple0(read_f32(raw, 0u), read_f32(raw, 0u + 4u)),
        ZeriumPropsTuple1(read_u32(raw, 8u), read_u32(raw, 8u + 4u)),
        ZeriumPropsTuple2(read_f32(raw, 16u), read_f32(raw, 16u + 4u)),
        ZeriumPropsTuple3(read_f32(raw, 24u), read_f32(raw, 24u + 4u)),
        read_f32(raw, 32u),
        read_f32(raw, 36u),
        read_bool(raw, 40u),
        vec4(read_f32(raw, 44u), read_f32(raw, 44u + 4u), read_f32(raw, 44u + 8u), read_f32(raw, 44u + 12u)),
        read_f32(raw, 60u),
        read_u32(raw, 64u),
        vec4(read_f32(raw, 68u), read_f32(raw, 68u + 4u), read_f32(raw, 68u + 8u), read_f32(raw, 68u + 12u)),
    );
}
