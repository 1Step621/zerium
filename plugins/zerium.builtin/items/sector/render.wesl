import package::generated::host::util::{DEG2RAD, scene_color};
import package::generated::sector::props;
import package::shared::geometry::{TAU, distance_coverage};
import package::shared::quad::placed_quad;

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
    output.instance_index = instance_index;
    return output;
}

@fragment
fn fragment_main(input: VertexOutput) -> @location(0) vec4<f32> {
    let properties = props(input.instance_index);
    let radius = length(input.local);
    var distance = radius - 1.0;
    if properties.inner_radius > 0.0 {
        distance = max(distance, properties.inner_radius / 100.0 - radius);
    }
    if properties.sweep_angle < 360.0 {
        let middle = (properties.start_angle + properties.sweep_angle * 0.5) * DEG2RAD;
        let angle = atan2(input.local.y, input.local.x) - middle;
        let wrapped = fract(angle / TAU + 0.5) * TAU - TAU * 0.5;
        let angular_distance = abs(wrapped) - properties.sweep_angle * DEG2RAD * 0.5;
        distance = max(distance, angular_distance * radius);
    }
    let coverage = distance_coverage(distance, 0.00001)
        * select(0.0, 1.0, properties.sweep_angle > 0.0);
    return scene_color(properties.color) * coverage;
}
