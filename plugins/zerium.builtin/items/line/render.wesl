import package::generated::host::item::to_ndc;
import package::generated::host::util::{DEG2RAD, rotate, scene_color};
import package::generated::line::props;
import package::shared::quad::placed_quad;

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) local: vec2<f32>,
    @location(1) color: vec4<f32>,
    @location(2) half_size: vec2<f32>,
    @location(3) @interpolate(flat) round_ends: u32,
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
    let half_size = size * 0.5;
    let rotated = rotate(quad.corner * half_size, properties.angle * DEG2RAD);
    var output: VertexOutput;
    output.position = vec4(to_ndc(instance_index, quad.center + rotated), 0.0, 1.0);
    output.local = quad.corner;
    output.color = scene_color(properties.color);
    output.half_size = half_size;
    output.round_ends = select(0u, 1u, properties.round_ends);
    return output;
}

@fragment
fn fragment_main(input: VertexOutput) -> @location(0) vec4<f32> {
    if input.round_ends != 0u {
        let radius = min(input.half_size.x, input.half_size.y);
        let segment_half_length = max(input.half_size.x - radius, 0.0);
        let point = input.local * input.half_size;
        let distance = length(vec2(max(abs(point.x) - segment_half_length, 0.0), point.y))
            - radius;
        if distance > 0.0 {
            discard;
        }
    }
    return input.color;
}
