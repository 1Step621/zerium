const PI: f32 = 3.141592653589793;
const MAX_STAR_VERTICES: u32 = 32u;

struct StarVertexOutput {
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
) -> StarVertexOutput {
    let params = zerium_load_parameters(instance_index);
    let position = vec2(params.position.v0, params.position.v1);
    let size = vec2(params.size.v0, params.size.v1);
    let local = zerium_item_quad_corner(vertex_index);

    var output: StarVertexOutput;
    output.position = zerium_item_quad_clip_position(
        instance_index,
        position,
        local,
        size,
    );
    output.local = local;
    output.color = zerium_scene_premultiplied_color(params.color);
    output.points = clamp(params.points, 3u, 16u);
    output.inner_radius = clamp(params.inner_radius, 0.05, 0.95);
    output.rotation = params.rotation * PI / 180.0;
    return output;
}

fn star_vertex(index: u32, points: u32, inner_radius: f32, rotation: f32) -> vec2<f32> {
    let angle = -PI * 0.5 + rotation + f32(index) * PI / f32(points);
    let radius = select(1.0, inner_radius, index % 2u == 1u);
    return vec2(cos(angle), sin(angle)) * radius;
}

@fragment
fn fragment_main(input: StarVertexOutput) -> @location(0) vec4<f32> {
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
    }
    if !inside {
        discard;
    }
    return input.color;
}
