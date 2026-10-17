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

struct ZeriumProps {
    position: ZeriumPropsTuple0,
    origin: ZeriumPropsTuple1,
    size: ZeriumPropsTuple2,
    angle: f32,
    fill_enabled: bool,
    color: vec4<f32>,
    outline_width: f32,
    outline_position: u32,
    outline_color: vec4<f32>,
    round_ends: bool,
}

fn props(instance_index: u32) -> ZeriumProps {
    let raw = item_props(instance_index);
    return ZeriumProps(
        ZeriumPropsTuple0(read_f32(raw, 0u), read_f32(raw, 0u + 4u)),
        ZeriumPropsTuple1(read_u32(raw, 8u), read_u32(raw, 8u + 4u)),
        ZeriumPropsTuple2(read_f32(raw, 16u), read_f32(raw, 16u + 4u)),
        read_f32(raw, 24u),
        read_bool(raw, 28u),
        vec4(read_f32(raw, 32u), read_f32(raw, 32u + 4u), read_f32(raw, 32u + 8u), read_f32(raw, 32u + 12u)),
        read_f32(raw, 48u),
        read_u32(raw, 52u),
        vec4(read_f32(raw, 56u), read_f32(raw, 56u + 4u), read_f32(raw, 56u + 8u), read_f32(raw, 56u + 12u)),
        read_bool(raw, 72u),
    );
}
