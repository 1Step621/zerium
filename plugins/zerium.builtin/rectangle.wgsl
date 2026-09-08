struct RectangleVertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) local: vec2<f32>,
    @location(1) color: vec4<f32>,
    @location(2) half_size: vec2<f32>,
    @location(3) @interpolate(flat) corner_radius: f32,
};

@vertex
fn vertex_main(
    @builtin(vertex_index) vertex_index: u32,
    @builtin(instance_index) instance_index: u32,
) -> RectangleVertexOutput {
    let params = zerium_load_parameters(instance_index);
    let position = vec2(params.position.v0, params.position.v1);
    let size = vec2(params.size.v0, params.size.v1);
    let half_size = size * 0.5;
    let local = zerium_item_quad_corner(vertex_index);

    var output: RectangleVertexOutput;
    output.position = zerium_item_quad_clip_position(
        instance_index,
        position,
        local,
        size,
    );
    output.local = local;
    output.color = zerium_scene_premultiplied_color(params.color);
    output.half_size = half_size;
    output.corner_radius = max(params.corner_radius, 0.0);
    return output;
}

@fragment
fn fragment_main(input: RectangleVertexOutput) -> @location(0) vec4<f32> {
    let radius = min(input.corner_radius, min(input.half_size.x, input.half_size.y));
    let point = input.local * input.half_size;
    let corner = abs(point) - (input.half_size - vec2(radius));
    let distance = length(max(corner, vec2(0.0))) +
        min(max(corner.x, corner.y), 0.0) - radius;
    if distance > 0.0 {
        discard;
    }
    return input.color;
}
