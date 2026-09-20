import package::generated::util::scene_color;
import package::generated::item::{quad_corner, quad_uv, quad_position};
import package::generated::properties_polygon::{props, get_points};

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
    let properties = props(instance_index);
    let position = vec2(properties.position.v0, properties.position.v1);
    let size = vec2(properties.size.v0, properties.size.v1);
    let corner = quad_corner(vertex_index);
    let local = quad_uv(corner);

    var output: PolygonVertexOutput;
    output.position = quad_position(
        instance_index,
        position,
        corner,
        size,
    );
    output.local = local;
    output.color = scene_color(properties.color);
    output.instance_index = instance_index;
    return output;
}

@fragment
fn fragment_main(input: PolygonVertexOutput) -> @location(0) vec4<f32> {
    let properties = props(input.instance_index);
    if properties.points_len < 3u {
        discard;
    }

    let previous_tuple = get_points(properties, properties.points_len - 1u);
    var previous = vec2(previous_tuple.v0, previous_tuple.v1) / 100.0;
    var inside = false;
    for (var index = 0u; index < properties.points_len; index += 1u) {
        let current_tuple = get_points(properties, index);
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
