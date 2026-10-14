import package::generated::host::util::scene_color;
import package::generated::star::props;
import package::shared::geometry::{PI, crosses_ray};
import package::shared::quad::placed_quad;

const MAX_STAR_VERTICES: u32 = 32u;

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) local: vec2<f32>,
    @location(1) color: vec4<f32>,
    @location(2) @interpolate(flat) points: u32,
    @location(3) @interpolate(flat) inner_radius: f32,
    @location(4) @interpolate(flat) rotation: f32,
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
    output.local = quad.corner;
    output.color = scene_color(properties.color);
    output.points = clamp(properties.points, 3u, 16u);
    output.inner_radius = clamp(properties.inner_radius / 100.0, 0.05, 0.95);
    output.rotation = properties.rotation * PI / 180.0;
    return output;
}

fn star_vertex(index: u32, points: u32, inner_radius: f32, rotation: f32) -> vec2<f32> {
    let angle = -PI * 0.5 + rotation + f32(index) * PI / f32(points);
    let radius = select(1.0, inner_radius, index % 2u == 1u);
    return vec2(cos(angle), sin(angle)) * radius;
}

@fragment
fn fragment_main(input: VertexOutput) -> @location(0) vec4<f32> {
    let vertex_count = input.points * 2u;
    var previous = star_vertex(
        vertex_count - 1u,
        input.points,
        input.inner_radius,
        input.rotation,
    );
    var inside = false;
    for (var index = 0u; index < MAX_STAR_VERTICES; index += 1u) {
        if index < vertex_count {
            let current = star_vertex(index, input.points, input.inner_radius, input.rotation);
            if crosses_ray(input.local, previous, current) {
                inside = !inside;
            }
            previous = current;
        }
    }
    if !inside {
        discard;
    }
    return input.color;
}
