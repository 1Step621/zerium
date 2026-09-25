import package::generated::util::scene_color;
import package::generated::item::{quad_corner, quad_position};
import package::generated::properties_ellipse::props;

struct EllipseVertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) local: vec2<f32>,
    @location(1) color: vec4<f32>,
};

@vertex
fn vertex_main(
    @builtin(vertex_index) vertex_index: u32,
    @builtin(instance_index) instance_index: u32,
) -> EllipseVertexOutput {
    let properties = props(instance_index);
    let position = vec2(properties.position.v0, properties.position.v1);
    let diameter = vec2(properties.diameter.v0, properties.diameter.v1);
    let local = quad_corner(vertex_index);

    var output: EllipseVertexOutput;
    output.position = quad_position(
        instance_index,
        position,
        local,
        diameter,
    );
    output.local = local;
    output.color = scene_color(properties.color);
    return output;
}

@fragment
fn fragment_main(input: EllipseVertexOutput) -> @location(0) vec4<f32> {
    if dot(input.local, input.local) > 1.0 {
        discard;
    }
    return input.color;
}
