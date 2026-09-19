import package::generated::util::{ZeriumContext, make_context};

struct ZeriumCompute {
    property_offset: u32,
    property_size: u32,
    width: u32,
    height: u32,
    composition_size: vec2<f32>,
    _padding_0: vec2<u32>,
};

@group(0) @binding(0)
var effect_input: texture_2d<f32>;

@group(0) @binding(1)
var effect_sampler: sampler;

@group(0) @binding(2)
var<uniform> compute_info: ZeriumCompute;

@group(0) @binding(3)
var<storage, read> property_words: array<u32>;

// Compute effects write scene-linear values directly into the pooled ping-pong
// texture. This avoids an RGBA8 quantization, a full-frame buffer, and a copy.
@group(0) @binding(4)
var effect_output: texture_storage_2d<rgba16float, write>;

// Input captured once at the beginning of the current regular pass chain.
@group(0) @binding(5)
var effect_source: texture_2d<f32>;

fn effect_props() -> ZeriumRawProps {
    return ZeriumRawProps(
        compute_info.property_offset,
        compute_info.property_size,
    );
}

fn context() -> ZeriumContext {
    let output_size = vec2<f32>(
        vec2(compute_info.width, compute_info.height),
    );
    return make_context(output_size, compute_info.composition_size);
}

fn store(position: vec2<u32>, color: vec4<f32>) {
    if position.x >= compute_info.width || position.y >= compute_info.height {
        return;
    }
    textureStore(effect_output, position, color);
}

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
