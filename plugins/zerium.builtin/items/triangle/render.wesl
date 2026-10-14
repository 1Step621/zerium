import package::generated::host::util::scene_color;
import package::generated::triangle::props;
import package::shared::geometry::cross_2d;
import package::shared::quad::placed_quad;

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) local: vec2<f32>,
    @location(1) color: vec4<f32>,
    @location(2) @interpolate(flat) apex_position: f32,
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
    output.apex_position = clamp(properties.apex_position / 100.0, 0.0, 1.0) * 2.0 - 1.0;
    return output;
}

@fragment
fn fragment_main(input: VertexOutput) -> @location(0) vec4<f32> {
    let apex = vec2(input.apex_position, -1.0);
    let left = vec2(-1.0, 1.0);
    let right = vec2(1.0, 1.0);
    let edge_a = cross_2d(input.local - apex, left - apex);
    let edge_b = cross_2d(input.local - left, right - left);
    let edge_c = cross_2d(input.local - right, apex - right);
    let all_positive = edge_a >= 0.0 && edge_b >= 0.0 && edge_c >= 0.0;
    let all_negative = edge_a <= 0.0 && edge_b <= 0.0 && edge_c <= 0.0;
    if !(all_positive || all_negative) {
        discard;
    }
    return input.color;
}
