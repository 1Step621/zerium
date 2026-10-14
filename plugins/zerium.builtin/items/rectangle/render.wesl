import package::generated::host::util::scene_color;
import package::generated::rectangle::props;
import package::shared::quad::placed_quad;

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) local: vec2<f32>,
    @location(1) color: vec4<f32>,
    @location(2) half_size: vec2<f32>,
    @location(3) @interpolate(flat) corner_radius: f32,
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
    output.half_size = size * 0.5;
    output.corner_radius = max(properties.corner_radius, 0.0);
    return output;
}

@fragment
fn fragment_main(input: VertexOutput) -> @location(0) vec4<f32> {
    let radius = min(input.corner_radius, min(input.half_size.x, input.half_size.y));
    let point = input.local * input.half_size;
    let corner = abs(point) - (input.half_size - vec2(radius));
    let distance = length(max(corner, vec2(0.0))) +
        min(max(corner.x, corner.y), 0.0) - radius;
    if distance > 0.0 {
        discard;
    }
    return input.color;
}
