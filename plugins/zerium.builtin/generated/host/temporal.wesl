import package::generated::host::_props::_temporal_state;
import package::generated::host::_context::make_surface_context;
import package::generated::host::util::ZeriumContext;

@group(0) @binding(0)
var temporal_sample: texture_2d<f32>;

@group(0) @binding(1)
var temporal_accumulation: texture_2d<f32>;

@group(0) @binding(2)
var temporal_sampler: sampler;

struct ZeriumTemporalInfo {
    sample_index: u32,
    sample_count: u32,
    frame_offset: f32,
    sample_progress: f32,
};

fn info() -> ZeriumTemporalInfo {
    return ZeriumTemporalInfo(
        _temporal_state.sample_index,
        _temporal_state.sample_count,
        _temporal_state.frame_offset,
        _temporal_state.sample_progress,
    );
}

fn context() -> ZeriumContext {
    let output_size = vec2<f32>(textureDimensions(temporal_sample));
    return make_surface_context(output_size, _temporal_state.composition_size, _temporal_state.surface_size);
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
