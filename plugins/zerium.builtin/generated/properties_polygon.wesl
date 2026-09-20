import package::generated::item::{ZeriumRawProps, item_props, read_u32, read_i32, read_f32, read_bool};

struct ZeriumPropsTuple0 {
    v0: f32,
    v1: f32,
};

struct ZeriumPropsTuple1 {
    v0: f32,
    v1: f32,
};

struct ZeriumPropsTuple2 {
    v0: f32,
    v1: f32,
};

struct ZeriumProps {
    _raw: ZeriumRawProps,
    position: ZeriumPropsTuple0,
    size: ZeriumPropsTuple1,
    points_len: u32,
    color: vec4<f32>,
};

fn props(instance_index: u32) -> ZeriumProps {
    let raw = item_props(instance_index);
    return ZeriumProps(raw, ZeriumPropsTuple0(read_f32(raw, 0u), read_f32(raw, 0u + 4u)), ZeriumPropsTuple1(read_f32(raw, 8u), read_f32(raw, 8u + 4u)), read_u32(raw, 20u), vec4(read_f32(raw, 24u), read_f32(raw, 24u + 4u), read_f32(raw, 24u + 8u), read_f32(raw, 24u + 12u)));
}

fn get_points(properties: ZeriumProps, index: u32) -> ZeriumPropsTuple2 {
    if index >= properties.points_len { return ZeriumPropsTuple2(0.0, 0.0); }
    let byte_offset = read_u32(properties._raw, 16u) + index * 8u;
    return ZeriumPropsTuple2(read_f32(properties._raw, byte_offset), read_f32(properties._raw, byte_offset + 4u));
}
