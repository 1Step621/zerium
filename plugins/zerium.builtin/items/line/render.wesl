import package::generated::host::item::to_ndc;
import package::generated::support::util::{DEG2RAD, rotate};
import package::generated::line::props;
import package::helpers::shape::{outlined_quad, shape_color};

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
    let half_size = size * 0.5;
    let rotated = rotate(quad.corner * half_size, properties.angle * DEG2RAD);
    var output: VertexOutput;
    output.position = vec4(to_ndc(quad.center + rotated), 0.0, 1.0);
    output.local = quad.corner;
    output.instance_index = instance_index;
    return output;
}

@fragment
fn fragment_main(input: VertexOutput) -> @location(0) vec4<f32> {
    let properties = props(input.instance_index);
    let half_size = vec2(properties.size.v0, properties.size.v1) * 0.5;
    let point = input.local * half_size;
    let corner = abs(point) - half_size;
    var distance = length(max(corner, vec2(0.0))) + min(max(corner.x, corner.y), 0.0);

    if properties.round_ends {
        let radius = min(half_size.x, half_size.y);
        let segment_half_length = max(half_size.x - radius, 0.0);
        distance = length(vec2(max(abs(point.x) - segment_half_length, 0.0), point.y)) - radius;
    }

    return shape_color(
        distance,
        properties.fill_enabled,
        properties.color,
        properties.outline_width,
        properties.outline_position,
        properties.outline_color,
    );
}
