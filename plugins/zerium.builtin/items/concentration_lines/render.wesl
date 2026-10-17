import package::generated::support::util::{rotate, scene_color};
import package::generated::concentration_lines::props;
import package::helpers::geometry::{TAU, distance_coverage};
import package::helpers::noise::hash;
import package::helpers::quad::placed_quad;

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
    let quad = placed_quad(
        vertex_index,
        vec2(properties.position.v0, properties.position.v1),
        size,
        vec2(properties.origin.v0, properties.origin.v1),
    );
    var output: VertexOutput;
    output.position = quad.position;
    output.local = quad.corner * size * 0.5;
    output.instance_index = instance_index;
    return output;
}

@fragment
fn fragment_main(input: VertexOutput) -> @location(0) vec4<f32> {
    let properties = props(input.instance_index);
    let radius = length(vec2(properties.size.v0, properties.size.v1) * 0.5);
    let angular_step = TAU / f32(properties.count);
    let nearest = i32(floor(atan2(input.local.y, input.local.x) / angular_step + 0.5));
    let variation = properties.variation / 100.0;
    var distance = radius;

    // Neighbouring bins keep angular jitter continuous across bin boundaries.
    for(var neighbour = -1; neighbour <= 1; neighbour += 1) {
        let index = u32(
            ((nearest + neighbour) % i32(properties.count) + i32(properties.count))
            % i32(properties.count),
        );
        let seed = index * 1013u + properties.seed;
        let angle = (f32(index) + (hash(seed) - 0.5) * variation * 0.8) * angular_step;
        let local = rotate(input.local, -angle);
        let start = radius * clamp(
            properties.inner_radius / 100.0 + (hash(seed + 1u) - 0.5) * variation * 0.25,
            0.0,
            1.0,
        );
        let width = properties.width * mix(1.0, 0.3 + hash(seed + 2u), variation);
        let taper = clamp((local.x - start) / max(radius - start, 1.0), 0.0, 1.0);
        distance = min(distance, max(abs(local.y) - width * 0.5 * taper, start - local.x));
    }

    let coverage = distance_coverage(distance, 0.001);
    return scene_color(properties.color) * coverage;
}
