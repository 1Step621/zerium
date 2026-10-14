import package::generated::host::effect::{
    ZeriumVertex,
    effect_input,
    effect_sampler,
    straight,
    vertex,
};
import package::generated::host::util::srgb;
import package::generated::posterize::props;
import package::shared::color::{display_rgb, quantize_alpha, quantize_rgb};

@vertex
fn vertex_main(@builtin(vertex_index) vertex_index: u32) -> ZeriumVertex {
    return vertex(vertex_index);
}

@fragment
fn fragment_main(input: ZeriumVertex) -> @location(0) vec4<f32> {
    let properties = props();
    let source = textureSample(effect_input, effect_sampler, input.uv);
    let linear = straight(source);
    let encoded = display_rgb(linear, properties.grayscale);
    let steps = f32(properties.levels - 1u);
    let quantized = quantize_rgb(encoded, steps, 0.5);
    let alpha = select(
        source.a,
        quantize_alpha(source.a, steps),
        properties.quantize_alpha,
    );
    return vec4(srgb(quantized) * alpha, alpha);
}
