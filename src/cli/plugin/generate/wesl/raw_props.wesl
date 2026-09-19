struct ZeriumRawProps {
    offset: u32,
    size: u32,
};

fn read_word(properties: ZeriumRawProps, word_index: u32) -> u32 {
    if word_index * 4u >= properties.size {
        return 0u;
    }
    return property_words[properties.offset + word_index];
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
