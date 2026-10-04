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

struct ZeriumPropsTuple3 {
    v0: f32,
    v1: f32,
};

struct ZeriumProps {
    _raw: ZeriumRawProps,
    position: ZeriumPropsTuple0,
    origin: ZeriumPropsTuple1,
    size: ZeriumPropsTuple2,
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
    return ZeriumProps(raw, ZeriumPropsTuple0(read_f32(raw, 0u), read_f32(raw, 0u + 4u)), ZeriumPropsTuple1(read_u32(raw, 8u), read_u32(raw, 8u + 4u)), ZeriumPropsTuple2(read_f32(raw, 16u), read_f32(raw, 16u + 4u)), read_u32(raw, 28u), read_f32(raw, 32u), read_f32(raw, 36u), vec4(read_f32(raw, 40u), read_f32(raw, 40u + 4u), read_f32(raw, 40u + 8u), read_f32(raw, 40u + 12u)), read_bool(raw, 56u), read_bool(raw, 60u), vec4(read_f32(raw, 64u), read_f32(raw, 64u + 4u), read_f32(raw, 64u + 8u), read_f32(raw, 64u + 12u)), read_f32(raw, 80u), read_f32(raw, 84u));
}

fn get_points(properties: ZeriumProps, index: u32) -> ZeriumPropsTuple3 {
    if index >= properties.points_len { return ZeriumPropsTuple3(0.0, 0.0); }
    let byte_offset = read_u32(properties._raw, 24u) + index * 8u;
    return ZeriumPropsTuple3(read_f32(properties._raw, byte_offset), read_f32(properties._raw, byte_offset + 4u));
}
