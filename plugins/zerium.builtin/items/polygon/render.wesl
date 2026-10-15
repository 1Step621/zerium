import package::generated::host::item::quad_uv;
import package::generated::polygon::{get_points, props};
import package::helpers::geometry::{crosses_ray, segment_distance};
import package::helpers::shape::{outlined_quad, shape_color};

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) local: vec2<f32>,
    @location(1) @interpolate(flat) instance_index: u32,
};

@vertex
fn vertex_main(
    @builtin(vertex_index) vertex_index: u32,
    @builtin(instance_index) instance_index: u32,
) -> VertexOutput {
    let properties = props(instance_index);
    let size = vec2(properties.size.v0, properties.size.v1);
    let quad = outlined_quad(
        vertex_index,
        instance_index,
        vec2(properties.position.v0, properties.position.v1),
        size,
        vec2(properties.origin.v0, properties.origin.v1),
        properties.outline_width,
        properties.outline_position,
    );
    var output: VertexOutput;
    output.position = quad.position;
    output.local = quad_uv(quad.corner);
    output.instance_index = instance_index;
    return output;
}

@fragment
fn fragment_main(input: VertexOutput) -> @location(0) vec4<f32> {
    let properties = props(input.instance_index);
    if properties.points_len < 3u {
        return vec4(0.0);
    }
    let size = vec2(properties.size.v0, properties.size.v1);
    let point = input.local * size;
    let previous_tuple = get_points(properties, properties.points_len - 1u);
    var previous = vec2(previous_tuple.v0, previous_tuple.v1) / 100.0 * size;
    var inside = false;
    var closest = 1e20;
    for (var index = 0u; index < properties.points_len; index += 1u) {
        let current_tuple = get_points(properties, index);
        let current = vec2(current_tuple.v0, current_tuple.v1) / 100.0 * size;
        if crosses_ray(point, previous, current) {
            inside = !inside;
        }
        closest = min(closest, segment_distance(point, previous, current));
        previous = current;
    }
    let distance = select(closest, -closest, inside);
    return shape_color(distance, properties.fill_enabled, properties.color,
        properties.outline_width, properties.outline_position, properties.outline_color);
}
