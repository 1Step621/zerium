import package::generated::host::_props::_effect_instance;
import package::generated::host::_context::make_surface_context;
import package::generated::host::util::{ZeriumContext, srgb};

@group(0) @binding(0)
var effect_input: texture_2d<f32>;

@group(0) @binding(1)
var effect_sampler: sampler;

// The input captured at the beginning of the current regular pass chain. A
// temporal pass ends the preceding chain, so passes after it capture its result.
@group(0) @binding(4)
var effect_source: texture_2d<f32>;

fn context() -> ZeriumContext {
    let output_size = vec2<f32>(textureDimensions(effect_input));
    return make_surface_context(output_size, _effect_instance.composition_size, _effect_instance.surface_size);
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
    return _effect_instance.surface_min + uv * _effect_instance.surface_size;
}

fn position_to_uv(position: vec2<f32>) -> vec2<f32> {
    return (position - _effect_instance.input_min) / _effect_instance.input_size;
}

fn input_uv_to_position(uv: vec2<f32>) -> vec2<f32> {
    return _effect_instance.input_min + uv * _effect_instance.input_size;
}

fn viewport_uv(uv: vec2<f32>) -> vec2<f32> {
    return uv_to_position(uv) / _effect_instance.composition_size + vec2(0.5);
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

fn over(foreground: vec4<f32>, background: vec4<f32>) -> vec4<f32> {
    return foreground + background * (1.0 - foreground.a);
}

fn premul(color: vec4<f32>, coverage: f32) -> vec4<f32> {
    let alpha = clamp(coverage, 0.0, 1.0) * color.a;
    return vec4(srgb(color.rgb) * alpha, alpha);
}
