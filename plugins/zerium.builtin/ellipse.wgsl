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
    let params = zerium_load_parameters(instance_index);
    let position = vec2(params.position.v0, params.position.v1);
    let radius = vec2(params.radius.v0, params.radius.v1);
    let local = zerium_item_quad_corner(vertex_index);

    var output: EllipseVertexOutput;
    output.position = zerium_item_quad_clip_position(
        instance_index,
        position,
        local,
        radius * 2.0,
    );
    output.local = local;
    output.color = zerium_scene_premultiplied_color(params.color);
    return output;
}

@fragment
fn fragment_main(input: EllipseVertexOutput) -> @location(0) vec4<f32> {
    if dot(input.local, input.local) > 1.0 {
        discard;
    }
    return input.color;
}
