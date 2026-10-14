import package::generated::host::effect::{
    ZeriumVertex, effect_input, effect_sampler, straight, vertex,
};
import package::generated::host::util::srgb;
import package::generated::hue_saturation::props;
import package::shared::color::{encode_srgb, hsl_to_rgb, rgb_to_hsl};

@vertex
fn vertex_main(@builtin(vertex_index) vertex_index: u32) -> ZeriumVertex {
    return vertex(vertex_index);
}

@fragment
fn fragment_main(input: ZeriumVertex) -> @location(0) vec4<f32> {
    let properties = props();
    let source = textureSample(effect_input, effect_sampler, input.uv);
    let hue_shift = fract(properties.hue / 360.0);
    if hue_shift == 0.0 && properties.saturation == 100.0 && properties.lightness == 0.0 {
        return source;
    }
    let encoded = clamp(encode_srgb(straight(source)), vec3(0.0), vec3(1.0));
    let hsl = rgb_to_hsl(encoded);
    let lightness = properties.lightness / 100.0;
    let adjusted = vec3(
        fract(hsl.x + hue_shift),
        clamp(hsl.y * properties.saturation / 100.0, 0.0, 1.0),
        select(hsl.z * (1.0 + lightness), mix(hsl.z, 1.0, lightness), lightness >= 0.0),
    );
    return vec4(srgb(hsl_to_rgb(adjusted)) * source.a, source.a);
}
