import package::generated::host::_props::{ZeriumRawProps, item_props, read_u32, read_i32, read_f32, read_bool};

struct ZeriumStr {
    _raw: ZeriumRawProps,
    _offset: u32,
    byte_len: u32,
};

fn str_byte(value: ZeriumStr, index: u32) -> u32 {
    if index >= value.byte_len { return 0u; }
    let byte_offset = value._offset + index;
    let word = read_u32(value._raw, byte_offset & 0xfffffffcu);
    return (word >> ((byte_offset & 3u) * 8u)) & 0xffu;
}

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
    _raw: ZeriumRawProps,
    position: ZeriumPropsTuple0,
    origin: ZeriumPropsTuple1,
    size: ZeriumPropsTuple2,
    value: f32,
    decimal_places: u32,
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
    return ZeriumProps(raw, ZeriumPropsTuple0(read_f32(raw, 0u), read_f32(raw, 0u + 4u)), ZeriumPropsTuple1(read_u32(raw, 8u), read_u32(raw, 8u + 4u)), ZeriumPropsTuple2(read_f32(raw, 16u), read_f32(raw, 16u + 4u)), read_f32(raw, 24u), read_u32(raw, 28u), read_u32(raw, 36u), read_f32(raw, 40u), vec4(read_f32(raw, 44u), read_f32(raw, 44u + 4u), read_f32(raw, 44u + 8u), read_f32(raw, 44u + 12u)), read_f32(raw, 60u), vec4(read_f32(raw, 64u), read_f32(raw, 64u + 4u), read_f32(raw, 64u + 8u), read_f32(raw, 64u + 12u)), read_u32(raw, 80u), read_u32(raw, 84u), read_bool(raw, 88u), read_bool(raw, 92u));
}

fn get_font_family(properties: ZeriumProps, index: u32) -> ZeriumStr {
    if index >= properties.font_family_len { return ZeriumStr(ZeriumRawProps(0u, 0u), 0u, 0u); }
    let byte_offset = read_u32(properties._raw, 32u) + index * 8u;
    return ZeriumStr(properties._raw, read_u32(properties._raw, byte_offset), read_u32(properties._raw, byte_offset + 4u));
}

@group(1) @binding(0)
var text: texture_2d<f32>;

@group(1) @binding(8)
var capability_sampler: sampler;
