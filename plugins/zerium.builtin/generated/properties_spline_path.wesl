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
    tension: f32,
    width: f32,
    color: vec4<f32>,
    closed: bool,
    fill_enabled: bool,
    fill_color: vec4<f32>,
    stroke_start: f32,
    stroke_end: f32,
};

fn props(instance_index: u32) -> ZeriumProps {
    let raw = item_props(instance_index);
    return ZeriumProps(raw, ZeriumPropsTuple0(read_f32(raw, 0u), read_f32(raw, 0u + 4u)), ZeriumPropsTuple1(read_f32(raw, 8u), read_f32(raw, 8u + 4u)), read_u32(raw, 20u), read_f32(raw, 24u), read_f32(raw, 28u), vec4(read_f32(raw, 32u), read_f32(raw, 32u + 4u), read_f32(raw, 32u + 8u), read_f32(raw, 32u + 12u)), read_bool(raw, 48u), read_bool(raw, 52u), vec4(read_f32(raw, 56u), read_f32(raw, 56u + 4u), read_f32(raw, 56u + 8u), read_f32(raw, 56u + 12u)), read_f32(raw, 72u), read_f32(raw, 76u));
}

fn get_points(properties: ZeriumProps, index: u32) -> ZeriumPropsTuple2 {
    if index >= properties.points_len { return ZeriumPropsTuple2(0.0, 0.0); }
    let byte_offset = read_u32(properties._raw, 16u) + index * 8u;
    return ZeriumPropsTuple2(read_f32(properties._raw, byte_offset), read_f32(properties._raw, byte_offset + 4u));
}
