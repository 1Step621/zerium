import package::generated::host::effect::{
    ZeriumVertex,
    effect_input,
    effect_sampler,
    straight,
    uv_to_position,
    vertex,
};
import package::generated::host::util::srgb;
import package::generated::dither::props;
import package::helpers::color::{display_rgb, quantize_rgb};
import package::helpers::noise::cell_hash;

const BAYER: array<f32, 16> = array(
    0.0, 8.0, 2.0, 10.0,
    12.0, 4.0, 14.0, 6.0,
    3.0, 11.0, 1.0, 9.0,
    15.0, 7.0, 13.0, 5.0,
);

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
    let cell = vec2<i32>(floor(uv_to_position(input.uv) / properties.cell_size));
    let index = bitcast<u32>(cell.x) % 4u + (bitcast<u32>(cell.y) % 4u) * 4u;
    let ordered = (BAYER[index] + 0.5) / 16.0;
    let threshold = select(ordered, cell_hash(cell, properties.seed), properties.pattern == 1u);
    let steps = f32(properties.levels - 1u);
    let quantized = clamp(quantize_rgb(encoded, steps, threshold), vec3(0.0), vec3(1.0));
    return vec4(srgb(quantized) * source.a, source.a);
}
