import package::generated::util::{ZeriumContext, make_context};

struct ZeriumTemporal {
    property_offset: u32,
    property_size: u32,
    sample_index: u32,
    sample_count: u32,
    frame_offset: f32,
    sample_progress: f32,
    composition_size: vec2<f32>,
};

@group(0) @binding(0)
var temporal_sample: texture_2d<f32>;

@group(0) @binding(1)
var temporal_accumulation: texture_2d<f32>;

@group(0) @binding(2)
var temporal_sampler: sampler;

@group(0) @binding(3)
var<uniform> temporal_state: ZeriumTemporal;

@group(0) @binding(4)
var<storage, read> property_words: array<u32>;

fn effect_props() -> ZeriumRawProps {
    return ZeriumRawProps(
        temporal_state.property_offset,
        temporal_state.property_size,
    );
}

fn info() -> ZeriumTemporal {
    return temporal_state;
}

fn context() -> ZeriumContext {
    let output_size = vec2<f32>(textureDimensions(temporal_sample));
    return make_context(output_size, temporal_state.composition_size);
}

struct ZeriumVertex {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

fn vertex(vertex_index: u32) -> ZeriumVertex {
    let positions = array<vec2<f32>, 3>(
        vec2(-1.0, -1.0),
        vec2( 3.0, -1.0),
        vec2(-1.0,  3.0),
    );
    let position = positions[vertex_index];
    var output: ZeriumVertex;
    output.position = vec4(position, 0.0, 1.0);
    output.uv = position * vec2(0.5, -0.5) + vec2(0.5);
    return output;
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
