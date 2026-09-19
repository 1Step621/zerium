import package::generated::item::{ZeriumItem, items, property_words, item_props, context, to_ndc, quad_corner, quad_uv, quad_position, rotate, ZeriumRawProps, read_word, read_u32, read_i32, read_f32, read_bool};

struct ZeriumPropsTuple0 {
    v0: f32,
    v1: f32,
};

struct ZeriumPropsTuple1 {
    v0: f32,
    v1: f32,
};

struct ZeriumProps {
    _raw: ZeriumRawProps,
    position: ZeriumPropsTuple0,
    size: ZeriumPropsTuple1,
    points: u32,
    inner_radius: f32,
    rotation: f32,
    color: vec4<f32>,
};

fn props(instance_index: u32) -> ZeriumProps {
    let raw = item_props(instance_index);
    return ZeriumProps(raw, ZeriumPropsTuple0(read_f32(raw, 0u), read_f32(raw, 0u + 4u)), ZeriumPropsTuple1(read_f32(raw, 8u), read_f32(raw, 8u + 4u)), read_u32(raw, 16u), read_f32(raw, 20u), read_f32(raw, 24u), vec4(read_f32(raw, 28u), read_f32(raw, 28u + 4u), read_f32(raw, 28u + 8u), read_f32(raw, 28u + 12u)));
}
