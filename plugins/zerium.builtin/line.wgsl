struct LineVertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) local: vec2<f32>,
    @location(1) color: vec4<f32>,
    @location(2) half_size: vec2<f32>,
    @location(3) @interpolate(flat) round_ends: u32,
};

@vertex
fn vertex_main(
    @builtin(vertex_index) vertex_index: u32,
    @builtin(instance_index) instance_index: u32,
) -> LineVertexOutput {
    let params = zerium_load_parameters(instance_index);
    let position = vec2(params.position.v0, params.position.v1);
    let size = vec2(params.size.v0, params.size.v1);
    let local = zerium_item_quad_corner(vertex_index);
    let half_size = size * 0.5;
    let angle = params.angle * ZERIUM_DEGREES_TO_RADIANS;
    let rotated = zerium_rotate_2d(local * half_size, angle);

    var output: LineVertexOutput;
    output.position = vec4(
        zerium_composition_position_to_ndc(instance_index, position + rotated),
        0.0,
        1.0,
    );
    output.local = local;
    output.color = zerium_scene_premultiplied_color(params.color);
    output.half_size = half_size;
    output.round_ends = select(0u, 1u, params.round_ends);
    return output;
}

@fragment
fn fragment_main(input: LineVertexOutput) -> @location(0) vec4<f32> {
    if input.round_ends != 0u {
        let radius = min(input.half_size.x, input.half_size.y);
        let segment_half_length = max(input.half_size.x - radius, 0.0);
        let point = input.local * input.half_size;
        let distance = length(vec2(max(abs(point.x) - segment_half_length, 0.0), point.y))
            - radius;
        if distance > 0.0 {
            discard;
        }
    }
    return input.color;
}
