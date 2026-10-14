import package::generated::host::util::{DEG2RAD, scene_color};
import package::generated::regular_polygon::props;
import package::shared::geometry::{TAU, cross_2d, distance_coverage};
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
    let step = TAU / f32(properties.sides);
    let apothem = cos(step * 0.5);
    let rounding = min(properties.rounding / (min(properties.size.v0, properties.size.v1) * 0.5), apothem);
    let inner_radius = max(1.0 - rounding / apothem, 0.00001);
    let start = (properties.angle - 90.0) * DEG2RAD;
    var distance = 100.0;
    var inside = true;
    for (var index = 0u; index < properties.sides; index += 1u) {
        let angle = start + f32(index) * step;
        let a = vec2(cos(angle), sin(angle)) * inner_radius;
        let b = vec2(cos(angle + step), sin(angle + step)) * inner_radius;
        let edge = b - a;
        let local = input.local - a;
        let closest = local - edge * clamp(dot(local, edge) / max(dot(edge, edge), 0.00000001), 0.0, 1.0);
        distance = min(distance, length(closest));
        inside = inside && cross_2d(edge, local) >= 0.0;
    }
    let signed_distance = select(distance, -distance, inside) - rounding;
    return scene_color(properties.color) * distance_coverage(signed_distance, 0.00001);
}
