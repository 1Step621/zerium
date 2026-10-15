import package::generated::star::props;
import package::helpers::geometry::{PI, crosses_ray, segment_distance};
import package::helpers::shape::{outlined_quad, shape_color};

const MAX_STAR_VERTICES: u32 = 32u;

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

fn star_vertex(index: u32, points: u32, inner_radius: f32, rotation: f32) -> vec2<f32> {
    let angle = -PI * 0.5 + rotation + f32(index) * PI / f32(points);
    let radius = select(1.0, inner_radius, index % 2u == 1u);
    return vec2(cos(angle), sin(angle)) * radius;
}

@fragment
fn fragment_main(input: VertexOutput) -> @location(0) vec4<f32> {
    let properties = props(input.instance_index);
    let half_size = vec2(properties.size.v0, properties.size.v1) * 0.5;
    let points = clamp(properties.points, 3u, 16u);
    let inner_radius = clamp(properties.inner_radius / 100.0, 0.05, 0.95);
    let rotation = properties.rotation * PI / 180.0;
    let point = input.local * half_size;
    let vertex_count = points * 2u;
    var previous = star_vertex(vertex_count - 1u, points, inner_radius, rotation) * half_size;
    var inside = false;
    var closest = 1e20;
    for (var index = 0u; index < MAX_STAR_VERTICES; index += 1u) {
        if index < vertex_count {
            let current = star_vertex(index, points, inner_radius, rotation) * half_size;
            if crosses_ray(point, previous, current) {
                inside = !inside;
            }
            closest = min(closest, segment_distance(point, previous, current));
            previous = current;
        }
    }
    let distance = select(closest, -closest, inside);
    return shape_color(distance, properties.fill_enabled, properties.color,
        properties.outline_width, properties.outline_position, properties.outline_color);
}
