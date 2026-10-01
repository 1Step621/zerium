import package::generated::host::_props::_compute_info;
import package::generated::host::_context::make_surface_context;
import package::generated::host::util::ZeriumContext;

@group(0) @binding(0)
var effect_input: texture_2d<f32>;

@group(0) @binding(1)
var effect_sampler: sampler;

// Compute effects write scene-linear values directly into the pooled ping-pong
// texture. This avoids an RGBA8 quantization, a full-frame buffer, and a copy.
@group(0) @binding(4)
var effect_output: texture_storage_2d<rgba16float, write>;

// Input captured once at the beginning of the current regular pass chain.
@group(0) @binding(5)
var effect_source: texture_2d<f32>;

fn context() -> ZeriumContext {
    let output_size = vec2<f32>(
        vec2(_compute_info.width, _compute_info.height),
    );
    return make_surface_context(output_size, _compute_info.composition_size, _compute_info.surface_size);
}

fn store(position: vec2<u32>, color: vec4<f32>) {
    if position.x >= _compute_info.width || position.y >= _compute_info.height {
        return;
    }
    textureStore(effect_output, position, color);
}
