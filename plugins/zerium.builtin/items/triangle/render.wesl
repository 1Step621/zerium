import package::generated::triangle::props;
import package::helpers::geometry::{cross_2d, segment_distance};
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
    output.local = quad.corner;
    output.instance_index = instance_index;
    return output;
}

@fragment
fn fragment_main(input: VertexOutput) -> @location(0) vec4<f32> {
    let properties = props(input.instance_index);
    let half_size = vec2(properties.size.v0, properties.size.v1) * 0.5;
    let apex_x = clamp(properties.apex_position / 100.0, 0.0, 1.0) * 2.0 - 1.0;
    let apex = vec2(apex_x, -1.0) * half_size;
    let left = vec2(-1.0, 1.0) * half_size;
    let right = half_size;
    let point = input.local * half_size;
    let edge_a = cross_2d(point - apex, left - apex);
    let edge_b = cross_2d(point - left, right - left);
    let edge_c = cross_2d(point - right, apex - right);
    let inside = (edge_a >= 0.0 && edge_b >= 0.0 && edge_c >= 0.0)
        || (edge_a <= 0.0 && edge_b <= 0.0 && edge_c <= 0.0);
    let closest = min(segment_distance(point, apex, left),
        min(segment_distance(point, left, right), segment_distance(point, right, apex)));
    let distance = select(closest, -closest, inside);
    return shape_color(distance, properties.fill_enabled, properties.color,
        properties.outline_width, properties.outline_position, properties.outline_color);
}
