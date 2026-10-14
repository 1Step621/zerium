import package::generated::host::item::quad_uv;
import package::generated::host::util::scene_color;
import package::generated::polygon::{get_points, props};
import package::shared::geometry::crosses_ray;
import package::shared::quad::placed_quad;

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) local: vec2<f32>,
    @location(1) color: vec4<f32>,
    @location(2) @interpolate(flat) instance_index: u32,
};

@vertex
fn vertex_main(
    @builtin(vertex_index) vertex_index: u32,
    @builtin(instance_index) instance_index: u32,
) -> VertexOutput {
    let properties = props(instance_index);
    let size = vec2(properties.size.v0, properties.size.v1);
    let quad = placed_quad(
        vertex_index,
        instance_index,
        vec2(properties.position.v0, properties.position.v1),
        size,
        vec2(properties.origin.v0, properties.origin.v1),
    );
    var output: VertexOutput;
    output.position = quad.position;
    output.local = quad_uv(quad.corner);
    output.color = scene_color(properties.color);
    output.instance_index = instance_index;
    return output;
}

@fragment
fn fragment_main(input: VertexOutput) -> @location(0) vec4<f32> {
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
        if crosses_ray(input.local, previous, current) {
            inside = !inside;
        }
        previous = current;
    }
    if !inside {
        discard;
    }
    return input.color;
}
