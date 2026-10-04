import package::generated::host::_props::{ZeriumRawProps, item_props, read_u32, read_i32, read_f32, read_bool};

struct ZeriumPropsTuple0 {
    v0: f32,
    v1: f32,
};

struct ZeriumPropsTuple1 {
    v0: u32,
    v1: u32,
};

struct ZeriumPropsTuple2 {
    v0: f32,
    v1: f32,
};

struct ZeriumProps {
    position: ZeriumPropsTuple0,
    origin: ZeriumPropsTuple1,
    size: ZeriumPropsTuple2,
    angle: f32,
    color: vec4<f32>,
    round_ends: bool,
};

fn props(instance_index: u32) -> ZeriumProps {
    let raw = item_props(instance_index);
    return ZeriumProps(ZeriumPropsTuple0(read_f32(raw, 0u), read_f32(raw, 0u + 4u)), ZeriumPropsTuple1(read_u32(raw, 8u), read_u32(raw, 8u + 4u)), ZeriumPropsTuple2(read_f32(raw, 16u), read_f32(raw, 16u + 4u)), read_f32(raw, 24u), vec4(read_f32(raw, 28u), read_f32(raw, 28u + 4u), read_f32(raw, 28u + 8u), read_f32(raw, 28u + 12u)), read_bool(raw, 44u));
}
