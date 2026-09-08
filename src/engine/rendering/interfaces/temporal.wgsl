struct ZeriumTemporalInfo {
    params_offset: u32,
    params_size: u32,
    sample_index: u32,
    sample_count: u32,
    frame_offset: f32,
    exposure_progress: f32,
    composition_size: vec2<f32>,
};

@group(0) @binding(0)
var zerium_temporal_sample: texture_2d<f32>;

@group(0) @binding(1)
var zerium_temporal_accumulation: texture_2d<f32>;

@group(0) @binding(2)
var zerium_temporal_sampler: sampler;

@group(0) @binding(3)
var<uniform> zerium_temporal_state: ZeriumTemporalInfo;

@group(0) @binding(4)
var<storage, read> zerium_parameter_words: array<u32>;

fn zerium_raw_params_for_effect() -> ZeriumRawParams {
    return ZeriumRawParams(
        zerium_temporal_state.params_offset,
        zerium_temporal_state.params_size,
    );
}

fn zerium_temporal_info() -> ZeriumTemporalInfo {
    return zerium_temporal_state;
}

fn zerium_render_context() -> ZeriumRenderContext {
    let output_size = vec2<f32>(textureDimensions(zerium_temporal_sample));
    return zerium_make_render_context(output_size, zerium_temporal_state.composition_size);
}

struct ZeriumTemporalVertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

fn zerium_temporal_vertex(vertex_index: u32) -> ZeriumTemporalVertexOutput {
    let positions = array<vec2<f32>, 3>(
        vec2(-1.0, -1.0),
        vec2( 3.0, -1.0),
        vec2(-1.0,  3.0),
    );
    let position = positions[vertex_index];
    var output: ZeriumTemporalVertexOutput;
    output.position = vec4(position, 0.0, 1.0);
    output.uv = position * vec2(0.5, -0.5) + vec2(0.5);
    return output;
}
