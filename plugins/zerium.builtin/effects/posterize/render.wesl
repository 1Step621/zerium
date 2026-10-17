import package::generated::host::render::{ZeriumVertex, vertex};
import package::generated::host::effect::{uv_to_position, straight, sample_input};
import package::generated::support::util::srgb;
import package::generated::posterize::props;
import package::helpers::color::{display_rgb, quantize_alpha, quantize_rgb};

@vertex
fn vertex_main(@builtin(vertex_index) vertex_index: u32) -> ZeriumVertex {
    return vertex(vertex_index);
}

@fragment
fn fragment_main(input: ZeriumVertex) -> @location(0) vec4<f32> {
    let properties = props();
    let source = sample_input(uv_to_position(input.uv));
    let linear = straight(source);
    let encoded = display_rgb(linear, properties.grayscale);
    let steps = f32(properties.levels - 1u);
    let quantized = quantize_rgb(encoded, steps, 0.5);
    let alpha = select(source.a, quantize_alpha(source.a, steps), properties.quantize_alpha);
    return vec4(srgb(quantized) * alpha, alpha);
}
