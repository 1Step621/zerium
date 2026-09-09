struct TriangleVertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) local: vec2<f32>,
    @location(1) color: vec4<f32>,
    @location(2) @interpolate(flat) apex_position: f32,
};

@vertex
fn vertex_main(
    @builtin(vertex_index) vertex_index: u32,
    @builtin(instance_index) instance_index: u32,
) -> TriangleVertexOutput {
    let params = zerium_load_parameters(instance_index);
    let position = vec2(params.position.v0, params.position.v1);
    let size = vec2(params.size.v0, params.size.v1);
    let local = zerium_item_quad_corner(vertex_index);

    var output: TriangleVertexOutput;
    output.position = zerium_item_quad_clip_position(
        instance_index,
        position,
        local,
        size,
    );
    output.local = local;
    output.color = zerium_scene_premultiplied_color(params.color);
    output.apex_position = clamp(params.apex_position / 100.0, 0.0, 1.0) * 2.0 - 1.0;
    return output;
}

fn triangle_edge(a: vec2<f32>, b: vec2<f32>, point: vec2<f32>) -> f32 {
    return (point.x - a.x) * (b.y - a.y) -
        (point.y - a.y) * (b.x - a.x);
}

@fragment
fn fragment_main(input: TriangleVertexOutput) -> @location(0) vec4<f32> {
    let apex = vec2(input.apex_position, -1.0);
    let left = vec2(-1.0, 1.0);
    let right = vec2(1.0, 1.0);
    let edge_a = triangle_edge(apex, left, input.local);
    let edge_b = triangle_edge(left, right, input.local);
    let edge_c = triangle_edge(right, apex, input.local);
    let all_positive = edge_a >= 0.0 && edge_b >= 0.0 && edge_c >= 0.0;
    let all_negative = edge_a <= 0.0 && edge_b <= 0.0 && edge_c <= 0.0;
    if !(all_positive || all_negative) {
        discard;
    }
    return input.color;
}
