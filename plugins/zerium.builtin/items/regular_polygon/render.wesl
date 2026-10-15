import package::generated::host::util::DEG2RAD;
import package::generated::regular_polygon::props;
import package::helpers::geometry::{TAU, cross_2d, segment_distance};
import package::helpers::shape::{outline_outset, outlined_quad, shape_color};

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

@fragment
fn fragment_main(input: VertexOutput) -> @location(0) vec4<f32> {
    let properties = props(input.instance_index);
    let half_size = vec2(properties.size.v0, properties.size.v1) * 0.5;
    let step = TAU / f32(properties.sides);
    let apothem = cos(step * 0.5);
    let rounding = min(properties.rounding / min(half_size.x, half_size.y), apothem);
    let inner_radius = max(1.0 - rounding / apothem, 0.00001);
    let start = (properties.angle - 90.0) * DEG2RAD;
    var distance = -1e20;
    if rounding > 0.0 {
        // The rounded polygon is a smaller polygon plus a circle in local
        // space. Its supporting lines give a continuous distance in scene
        // pixels, even when nonuniform scaling turns its corners elliptical.
        // Sample normals finely enough to keep corner error below 0.05 px.
        let radius_pixels = rounding * max(half_size.x, half_size.y)
            + outline_outset(properties.outline_width, properties.outline_position);
        let steps = u32(max(ceil(step * sqrt(max(radius_pixels, 1.0) / 0.32)), 1.0));
        let normal_step = step / f32(steps);
        let advance = vec2(cos(normal_step), sin(normal_step));
        for (var index = 0u; index < properties.sides; index += 1u) {
            let angle = start + f32(index) * step;
            let vertex = vec2(cos(angle), sin(angle)) * inner_radius;
            var normal = vec2(cos(angle - step * 0.5), sin(angle - step * 0.5));
            for (var sample = 0u; sample <= steps; sample += 1u) {
                let plane = (dot(input.local - vertex, normal) - rounding) / length(normal / half_size);
                distance = max(distance, plane);
                normal = vec2(normal.x * advance.x - normal.y * advance.y,
                    normal.x * advance.y + normal.y * advance.x);
            }
        }
        return shape_color(distance, properties.fill_enabled, properties.color,
            properties.outline_width, properties.outline_position, properties.outline_color);
    }
    var pixel_distance = 1e20;
    var inside = true;
    for (var index = 0u; index < properties.sides; index += 1u) {
        let angle = start + f32(index) * step;
        let a = vec2(cos(angle), sin(angle)) * inner_radius;
        let b = vec2(cos(angle + step), sin(angle + step)) * inner_radius;
        let edge = b - a;
        let local = input.local - a;
        pixel_distance = min(pixel_distance, segment_distance(input.local * half_size, a * half_size, b * half_size));
        inside = inside && cross_2d(edge, local) >= 0.0;
    }
    distance = select(pixel_distance, -pixel_distance, inside);
    return shape_color(distance, properties.fill_enabled, properties.color,
        properties.outline_width, properties.outline_position, properties.outline_color);
}
