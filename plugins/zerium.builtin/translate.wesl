import package::generated::host::effect::{effect_input, ZeriumVertex, vertex, uv_to_position, position_to_uv, sample_or_transparent};
import package::generated::translate::props;

@vertex
fn vertex_main(@builtin(vertex_index) vertex_index: u32) -> ZeriumVertex {
    return vertex(vertex_index);
}

@fragment
fn fragment_main(input: ZeriumVertex) -> @location(0) vec4<f32> {
    let properties = props();
    let offset = vec2(properties.offset.v0, properties.offset.v1);
    let source_uv = position_to_uv(uv_to_position(input.uv) - offset);

    // Preserve exact texels when the mapped position lands on a texel center.
    // Test in source pixels so different surface origins, pixel-aligned bounds,
    // and preview resolutions also work with fractional movement.
    let source_size = vec2<f32>(textureDimensions(effect_input));
    let source_texel = source_uv * source_size - vec2(0.5);
    let rounded_texel = round(source_texel);
    let source_position = vec2<i32>(rounded_texel);
    let source_size_i = vec2<i32>(textureDimensions(effect_input));
    if all(abs(source_texel - rounded_texel) < vec2(0.0001))
        && all(source_position >= vec2<i32>(0, 0))
        && all(source_position < source_size_i)
    {
        return textureLoad(effect_input, source_position, 0);
    }
    return sample_or_transparent(source_uv);
}
