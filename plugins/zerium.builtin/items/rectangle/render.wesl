import package::generated::rectangle::props;
import package::helpers::shape::{outlined_quad, shape_color};
import package::helpers::geometry::rounded_rectangle_distance;

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) local: vec2<f32>,
    @interpolate(flat)
    @location(1) instance_index: u32,
}

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
    let distance = rounded_rectangle_distance(input.local * half_size, half_size, properties.corner_radius);
    return shape_color(
        distance,
        properties.fill_enabled,
        properties.color,
        properties.outline_width,
        properties.outline_position,
        properties.outline_color,
    );
}
