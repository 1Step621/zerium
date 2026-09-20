import package::generated::effect::{effect_input, context, ZeriumVertex, vertex, sample_or_transparent};
import package::generated::properties_translate::props;

@vertex
fn vertex_main(@builtin(vertex_index) vertex_index: u32) -> ZeriumVertex {
    return vertex(vertex_index);
}

@fragment
fn fragment_main(input: ZeriumVertex) -> @location(0) vec4<f32> {
    let properties = props();
    let offset = vec2(properties.offset.v0, properties.offset.v1);
    let source_uv = input.uv - offset / context().composition_size;

    // Preserve exact texels for zero and integer-pixel movement. Fractional
    // movement, or a preview scaled away from the composition resolution,
    // still uses the filtered path for smooth motion.
    let source_size = vec2<f32>(textureDimensions(effect_input));
    let pixel_offset = offset * source_size / context().composition_size;
    let rounded_offset = round(pixel_offset);
    let source_position = vec2<i32>(floor(source_uv * source_size));
    let source_size_i = vec2<i32>(textureDimensions(effect_input));
    if all(abs(pixel_offset - rounded_offset) < vec2(0.0001))
        && all(source_position >= vec2<i32>(0, 0))
        && all(source_position < source_size_i)
    {
        return textureLoad(effect_input, source_position, 0);
    }
    return sample_or_transparent(source_uv);
}
