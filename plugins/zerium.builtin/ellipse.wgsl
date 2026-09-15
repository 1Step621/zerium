struct EllipseVertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) local: vec2<f32>,
    @location(1) color: vec4<f32>,
};

@vertex
fn vertex_main(
    @builtin(vertex_index) vertex_index: u32,
    @builtin(instance_index) instance_index: u32,
) -> EllipseVertexOutput {
    let properties = zerium_load_properties(instance_index);
    let position = vec2(properties.position.v0, properties.position.v1);
    let radius = vec2(properties.radius.v0, properties.radius.v1);
    let local = zerium_item_quad_corner(vertex_index);

    var output: EllipseVertexOutput;
    output.position = zerium_item_quad_clip_position(
        instance_index,
        position,
        local,
        radius * 2.0,
    );
    output.local = local;
    output.color = zerium_scene_premultiplied_color(properties.color);
    return output;
}

@fragment
fn fragment_main(input: EllipseVertexOutput) -> @location(0) vec4<f32> {
    if dot(input.local, input.local) > 1.0 {
        discard;
    }
    return input.color;
}
