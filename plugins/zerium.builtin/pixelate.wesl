import package::generated::effect::{effect_input, effect_sampler, context, ZeriumVertex, vertex};
import package::generated::properties_pixelate::props;

@vertex
fn vertex_main(@builtin(vertex_index) vertex_index: u32) -> ZeriumVertex {
    return vertex(vertex_index);
}

@fragment
fn fragment_main(input: ZeriumVertex) -> @location(0) vec4<f32> {
    let properties = props();
    let dimensions = context().output_size;
    let block_size = max(round(properties.block_size * context().composition_scale), 1.0);
    let block_center = (floor(input.uv * dimensions / block_size) + vec2(0.5)) * block_size;
    let half_texel = vec2(0.5) / dimensions;
    let sample_uv = clamp(block_center / dimensions, half_texel, vec2(1.0) - half_texel);
    return textureSample(effect_input, effect_sampler, sample_uv);
}
