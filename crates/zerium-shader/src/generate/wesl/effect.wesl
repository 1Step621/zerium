import package::generated::_internal::effect_state::_pass_state;
import package::generated::_internal::context::make_surface_context;
import package::generated::support::util::{ZeriumContext, srgb};
import package::generated::_internal::input::{
    info as input_info,
    position as input_position,
    reference_uv as input_reference_uv,
    sample as sample_input,
    sample_uv as sample_input_uv,
    load as load_input
};
import package::generated::_internal::source::{
    info as source_info,
    position as source_position,
    reference_uv as source_reference_uv,
    sample as sample_source,
    sample_uv as sample_source_uv,
    load as load_source
};

fn context() -> ZeriumContext {
    return make_surface_context(
        vec2<f32>(_pass_state.output_size),
        _pass_state.composition_size,
        _pass_state.surface_size,
    );
}

fn uv_to_position(uv: vec2<f32>) -> vec2<f32> {
    return _pass_state.surface_min + uv * _pass_state.surface_size;
}

fn pixel_position(pixel: vec2<i32>) -> vec2<f32> {
    return uv_to_position((vec2<f32>(pixel) + vec2(0.5)) / vec2<f32>(_pass_state.output_size));
}

fn reference_uv(position: vec2<f32>) -> vec2<f32> {
    return (position - _pass_state.bounds_min) / max(_pass_state.bounds_size, vec2(0.000001));
}

fn uv_inside(uv: vec2<f32>) -> bool {
    return all(uv >= vec2(0.0)) && all(uv <= vec2(1.0));
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
