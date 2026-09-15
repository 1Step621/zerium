struct TextVertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vertex_main(
    @builtin(vertex_index) vertex_index: u32,
    @builtin(instance_index) instance_index: u32,
) -> TextVertexOutput {
    let properties = zerium_load_properties(instance_index);
    let position = vec2(properties.position.v0, properties.position.v1);
    let size = vec2(properties.size.v0, properties.size.v1);
    let corner = zerium_item_quad_corner(vertex_index);
    var output: TextVertexOutput;
    output.position = zerium_item_quad_clip_position(
        instance_index,
        position,
        corner,
        size,
    );
    output.uv = zerium_item_quad_uv(corner);
    return output;
}

@fragment
fn fragment_main(input: TextVertexOutput) -> @location(0) vec4<f32> {
    let color = textureSample(zerium_media_slot_0, zerium_media_sampler, input.uv);
    return vec4(color.rgb * color.a, color.a);
}
