struct PolygonVertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) local: vec2<f32>,
    @location(1) color: vec4<f32>,
    @location(2) @interpolate(flat) instance_index: u32,
};

@vertex
fn vertex_main(
    @builtin(vertex_index) vertex_index: u32,
    @builtin(instance_index) instance_index: u32,
) -> PolygonVertexOutput {
    let params = zerium_load_parameters(instance_index);
    let position = vec2(params.position.v0, params.position.v1);
    let size = vec2(params.size.v0, params.size.v1);
    let corner = zerium_item_quad_corner(vertex_index);
    let local = zerium_item_quad_uv(corner);

    var output: PolygonVertexOutput;
    output.position = zerium_item_quad_clip_position(
        instance_index,
        position,
        corner,
        size,
    );
    output.local = local;
    output.color = zerium_scene_premultiplied_color(params.color);
    output.instance_index = instance_index;
    return output;
}

@fragment
fn fragment_main(input: PolygonVertexOutput) -> @location(0) vec4<f32> {
    let params = zerium_load_parameters(input.instance_index);
    if params.points_len < 3u {
        discard;
    }

    let previous_tuple = zerium_parameter_points_get(params, params.points_len - 1u);
    var previous = vec2(previous_tuple.v0, previous_tuple.v1) / 100.0;
    var inside = false;
    for (var index = 0u; index < params.points_len; index += 1u) {
        let current_tuple = zerium_parameter_points_get(params, index);
        let current = vec2(current_tuple.v0, current_tuple.v1) / 100.0;
        let crosses = (current.y > input.local.y) != (previous.y > input.local.y);
        if crosses {
            let edge_x = (previous.x - current.x)
                * (input.local.y - current.y)
                / (previous.y - current.y)
                + current.x;
            if input.local.x < edge_x {
                inside = !inside;
            }
        }
        previous = current;
    }
    if !inside {
        discard;
    }
    return input.color;
}
