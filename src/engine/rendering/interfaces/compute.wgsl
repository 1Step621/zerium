struct ZeriumComputeInfo {
    params_offset: u32,
    params_size: u32,
    width: u32,
    height: u32,
    composition_size: vec2<f32>,
    _padding_0: vec2<u32>,
};

@group(0) @binding(0)
var zerium_effect_input: texture_2d<f32>;

@group(0) @binding(1)
var zerium_effect_sampler: sampler;

@group(0) @binding(2)
var<uniform> zerium_compute_info: ZeriumComputeInfo;

@group(0) @binding(3)
var<storage, read> zerium_parameter_words: array<u32>;

// Compute effects write scene-linear values directly into the pooled ping-pong
// texture. This avoids an RGBA8 quantization, a full-frame buffer, and a copy.
@group(0) @binding(4)
var zerium_effect_output: texture_storage_2d<rgba16float, write>;

// Input captured once at the beginning of the current regular pass chain.
@group(0) @binding(5)
var zerium_effect_source: texture_2d<f32>;

fn zerium_raw_params_for_effect() -> ZeriumRawParams {
    return ZeriumRawParams(
        zerium_compute_info.params_offset,
        zerium_compute_info.params_size,
    );
}

fn zerium_render_context() -> ZeriumRenderContext {
    let output_size = vec2<f32>(
        vec2(zerium_compute_info.width, zerium_compute_info.height),
    );
    return zerium_make_render_context(output_size, zerium_compute_info.composition_size);
}

fn zerium_store_output(position: vec2<u32>, color: vec4<f32>) {
    if position.x >= zerium_compute_info.width || position.y >= zerium_compute_info.height {
        return;
    }
    textureStore(zerium_effect_output, position, color);
}
