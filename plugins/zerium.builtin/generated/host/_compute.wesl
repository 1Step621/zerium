// Internal GPU ABI. Plugin shaders use the typed generated property API.

struct _ZeriumCompute {
    property_offset: u32,
    property_size: u32,
    width: u32,
    height: u32,
    composition_size: vec2<f32>,
    surface_min: vec2<f32>,
    surface_size: vec2<f32>,
};

@group(0) @binding(2)
var<uniform> _compute_info: _ZeriumCompute;

@group(0) @binding(3)
var<storage, read> _property_words: array<u32>;

fn effect_props() -> ZeriumRawProps {
    return ZeriumRawProps(
        _compute_info.property_offset,
        _compute_info.property_size,
    );
}

struct ZeriumRawProps {
    offset: u32,
    size: u32,
};

fn read_word(properties: ZeriumRawProps, word_index: u32) -> u32 {
    if word_index * 4u >= properties.size {
        return 0u;
    }
    return _property_words[properties.offset + word_index];
}

fn read_u32(properties: ZeriumRawProps, byte_offset: u32) -> u32 {
    if byte_offset + 4u > properties.size {
        return 0u;
    }
    return read_word(properties, byte_offset / 4u);
}

fn read_i32(properties: ZeriumRawProps, byte_offset: u32) -> i32 {
    return bitcast<i32>(read_u32(properties, byte_offset));
}

fn read_f32(properties: ZeriumRawProps, byte_offset: u32) -> f32 {
    return bitcast<f32>(read_u32(properties, byte_offset));
}

fn read_bool(properties: ZeriumRawProps, byte_offset: u32) -> bool {
    return read_u32(properties, byte_offset) != 0u;
}
