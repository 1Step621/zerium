import package::generated::host::effect::{
    ZeriumVertex,
    context,
    effect_input,
    effect_sampler,
    sample_or_transparent,
    straight,
    vertex,
};
import package::generated::host::util::{DEG2RAD, srgb};
import package::generated::emboss::props;
import package::helpers::color::luminance;

@vertex
fn vertex_main(@builtin(vertex_index) vertex_index: u32) -> ZeriumVertex {
    return vertex(vertex_index);
}

@fragment
fn fragment_main(input: ZeriumVertex) -> @location(0) vec4<f32> {
    let properties = props();
    let direction = vec2(cos(properties.angle * DEG2RAD), sin(properties.angle * DEG2RAD));
    let offset = direction * properties.distance / context().surface_size;
    let source = textureSample(effect_input, effect_sampler, input.uv);
    let a = sample_or_transparent(input.uv - offset);
    let b = sample_or_transparent(input.uv + offset);
    let slope = luminance(a.rgb - b.rgb) * properties.strength / 100.0;
    let gray = srgb(vec3(clamp(0.5 + slope, 0.0, 1.0)));
    let colored = straight(source) * max(1.0 + slope, 0.0);
    return vec4(select(gray, colored, properties.preserve_color) * source.a, source.a);
}
