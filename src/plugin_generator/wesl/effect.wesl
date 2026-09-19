import package::generated::util::{ZeriumContext, make_context, srgb};

struct ZeriumEffect {
    property_offset: u32,
    property_size: u32,
    sample_index: u32,
    sample_count: u32,
    frame_offset: f32,
    exposure_progress: f32,
    composition_size: vec2<f32>,
};

@group(0) @binding(0)
var effect_input: texture_2d<f32>;

@group(0) @binding(1)
var effect_sampler: sampler;

@group(0) @binding(2)
var<uniform> effect_instance: ZeriumEffect;

@group(0) @binding(3)
var<storage, read> property_words: array<u32>;

// The input captured at the beginning of the current regular pass chain. A
// temporal pass ends the preceding chain, so passes after it capture its result.
@group(0) @binding(4)
var effect_source: texture_2d<f32>;

fn effect_props() -> ZeriumRawProps {
    return ZeriumRawProps(
        effect_instance.property_offset,
        effect_instance.property_size,
    );
}

fn context() -> ZeriumContext {
    let output_size = vec2<f32>(textureDimensions(effect_input));
    return make_context(output_size, effect_instance.composition_size);
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

fn uv_to_position(uv: vec2<f32>) -> vec2<f32> {
    return (uv - vec2(0.5)) * context().composition_size;
}

fn position_to_uv(position: vec2<f32>) -> vec2<f32> {
    return position / context().composition_size + vec2(0.5);
}

fn uv_inside(uv: vec2<f32>) -> bool {
    return all(uv >= vec2(0.0)) && all(uv <= vec2(1.0));
}

fn sample_or_transparent(uv: vec2<f32>) -> vec4<f32> {
    if !uv_inside(uv) {
        return vec4(0.0);
    }
    return textureSample(effect_input, effect_sampler, uv);
}

fn straight(color: vec4<f32>) -> vec3<f32> {
    if color.a <= 0.00001 {
        return vec3(0.0);
    }
    return color.rgb / color.a;
}

fn rotate(point: vec2<f32>, radians: f32) -> vec2<f32> {
    let sine = sin(radians);
    let cosine = cos(radians);
    return vec2(
        point.x * cosine - point.y * sine,
        point.x * sine + point.y * cosine,
    );
}

fn over(foreground: vec4<f32>, background: vec4<f32>) -> vec4<f32> {
    return foreground + background * (1.0 - foreground.a);
}

fn premul(color: vec4<f32>, coverage: f32) -> vec4<f32> {
    let alpha = clamp(coverage, 0.0, 1.0) * color.a;
    return vec4(srgb(color.rgb) * alpha, alpha);
}
