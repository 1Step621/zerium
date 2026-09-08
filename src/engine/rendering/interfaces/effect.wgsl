struct ZeriumEffectInstance {
    params_offset: u32,
    params_size: u32,
    sample_index: u32,
    sample_count: u32,
    frame_offset: f32,
    exposure_progress: f32,
    composition_size: vec2<f32>,
};

@group(0) @binding(0)
var zerium_effect_input: texture_2d<f32>;

@group(0) @binding(1)
var zerium_effect_sampler: sampler;

@group(0) @binding(2)
var<uniform> zerium_effect_instance: ZeriumEffectInstance;

@group(0) @binding(3)
var<storage, read> zerium_parameter_words: array<u32>;

// The input captured at the beginning of the current regular pass chain. A
// temporal pass ends the preceding chain, so passes after it capture its result.
@group(0) @binding(4)
var zerium_effect_source: texture_2d<f32>;

fn zerium_raw_params_for_effect() -> ZeriumRawParams {
    return ZeriumRawParams(
        zerium_effect_instance.params_offset,
        zerium_effect_instance.params_size,
    );
}

fn zerium_render_context() -> ZeriumRenderContext {
    let output_size = vec2<f32>(textureDimensions(zerium_effect_input));
    return zerium_make_render_context(output_size, zerium_effect_instance.composition_size);
}

struct ZeriumEffectVertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

fn zerium_effect_vertex(vertex_index: u32) -> ZeriumEffectVertexOutput {
    let positions = array<vec2<f32>, 3>(
        vec2(-1.0, -1.0),
        vec2( 3.0, -1.0),
        vec2(-1.0,  3.0),
    );
    let position = positions[vertex_index];
    var output: ZeriumEffectVertexOutput;
    output.position = vec4(position, 0.0, 1.0);
    output.uv = position * vec2(0.5, -0.5) + vec2(0.5);
    return output;
}

fn zerium_effect_uv_to_composition_position(uv: vec2<f32>) -> vec2<f32> {
    return (uv - vec2(0.5)) * zerium_render_context().composition_size;
}

fn zerium_composition_position_to_effect_uv(position: vec2<f32>) -> vec2<f32> {
    return position / zerium_render_context().composition_size + vec2(0.5);
}

fn zerium_effect_uv_is_inside(uv: vec2<f32>) -> bool {
    return all(uv >= vec2(0.0)) && all(uv <= vec2(1.0));
}

fn zerium_sample_effect_input_or_transparent(uv: vec2<f32>) -> vec4<f32> {
    if !zerium_effect_uv_is_inside(uv) {
        return vec4(0.0);
    }
    return textureSample(zerium_effect_input, zerium_effect_sampler, uv);
}

fn zerium_straight_rgb(color: vec4<f32>) -> vec3<f32> {
    if color.a <= 0.00001 {
        return vec3(0.0);
    }
    return color.rgb / color.a;
}

fn zerium_rotate_2d(point: vec2<f32>, radians: f32) -> vec2<f32> {
    let sine = sin(radians);
    let cosine = cos(radians);
    return vec2(
        point.x * cosine - point.y * sine,
        point.x * sine + point.y * cosine,
    );
}

fn zerium_composite_over(foreground: vec4<f32>, background: vec4<f32>) -> vec4<f32> {
    return foreground + background * (1.0 - foreground.a);
}

fn zerium_premultiplied_color(color: vec4<f32>, coverage: f32) -> vec4<f32> {
    let alpha = clamp(coverage, 0.0, 1.0) * color.a;
    return vec4(zerium_srgb_to_scene(color.rgb) * alpha, alpha);
}
