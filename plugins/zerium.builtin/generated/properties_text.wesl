import package::generated::item::{ZeriumRawProps, item_props, read_u32, read_i32, read_f32, read_bool};

struct ZeriumStr {
    _offset: u32,
    byte_len: u32,
};

fn str_byte(raw: ZeriumRawProps, value: ZeriumStr, index: u32) -> u32 {
    if index >= value.byte_len { return 0u; }
    let byte_offset = value._offset + index;
    let word = read_u32(raw, byte_offset & 0xfffffffcu);
    return (word >> ((byte_offset & 3u) * 8u)) & 0xffu;
}

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
    text: ZeriumStr,
    font_family_len: u32,
    font_size: f32,
    color: vec4<f32>,
    outline_width: f32,
    outline_color: vec4<f32>,
    horizontal_alignment: u32,
    vertical_alignment: u32,
    bold: bool,
    italic: bool,
};

fn props(instance_index: u32) -> ZeriumProps {
    let raw = item_props(instance_index);
    return ZeriumProps(raw, ZeriumPropsTuple0(read_f32(raw, 0u), read_f32(raw, 0u + 4u)), ZeriumPropsTuple1(read_f32(raw, 8u), read_f32(raw, 8u + 4u)), ZeriumStr(read_u32(raw, 16u), read_u32(raw, 16u + 4u)), read_u32(raw, 28u), read_f32(raw, 32u), vec4(read_f32(raw, 36u), read_f32(raw, 36u + 4u), read_f32(raw, 36u + 8u), read_f32(raw, 36u + 12u)), read_f32(raw, 52u), vec4(read_f32(raw, 56u), read_f32(raw, 56u + 4u), read_f32(raw, 56u + 8u), read_f32(raw, 56u + 12u)), read_u32(raw, 72u), read_u32(raw, 76u), read_bool(raw, 80u), read_bool(raw, 84u));
}

fn get_font_family(properties: ZeriumProps, index: u32) -> ZeriumStr {
    if index >= properties.font_family_len { return ZeriumStr(0u, 0u); }
    let byte_offset = read_u32(properties._raw, 24u) + index * 8u;
    return ZeriumStr(read_u32(properties._raw, byte_offset), read_u32(properties._raw, byte_offset + 4u));
}
