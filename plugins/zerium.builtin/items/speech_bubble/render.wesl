import package::helpers::surface::{SurfaceVertex, surface_vertex, surface_position};
import package::generated::speech_bubble::props;
import package::generated::host::item::placement_center;
import package::helpers::shape::shape_color;
import package::helpers::geometry::{segment_distance, cross_2d};

fn triangle_distance(point: vec2<f32>, a: vec2<f32>, b: vec2<f32>, c: vec2<f32>) -> f32 {
    let distance = min(
        segment_distance(point, a, b),
        min(segment_distance(point, b, c), segment_distance(point, c, a)),
    );
    let turns = vec3(cross_2d(b - a, point - a), cross_2d(c - b, point - b), cross_2d(a - c, point - c));
    let inside = all(turns >= vec3(0.0)) || all(turns <= vec3(0.0));
    return select(distance, -distance, inside);
}

// Chords between points on the rounded edge stay inside the body, keeping
// the tail attached even when the body has fully rounded corners.
fn side_point(along: f32, half_size: vec2<f32>, radius: f32) -> vec2<f32> {
    let corner = max(abs(along) - (half_size.y - radius), 0.0);
    let edge = half_size.x - radius + sqrt(max(radius * radius - corner * corner, 0.0));
    return vec2(edge, along);
}

@vertex
fn vertex_main(@builtin(vertex_index) index: u32, @builtin(instance_index) instance: u32) -> SurfaceVertex {
    return surface_vertex(index, instance);
}

@fragment
fn fragment_main(input: SurfaceVertex) -> @location(0) vec4<f32> {
    let p = props(input.instance_index);
    let position = surface_position(input);
    let center = placement_center(
        vec2(p.position.v0, p.position.v1),
        vec2(p.size.v0, p.size.v1),
        vec2(p.origin.v0, p.origin.v1),
    );
    let point = position - center;
    let half_size = vec2(p.size.v0, p.size.v1) * 0.5;
    let radius = min(p.corner_radius, min(half_size.x, half_size.y));
    let q = abs(point) - (half_size - vec2(radius));
    var distance = length(max(q, vec2(0.0))) + min(max(q.x, q.y), 0.0) - radius;
    let tip = vec2(p.tail.v0, p.tail.v1);
    if p.tail_width > 0.0 && any(abs(tip) > half_size) {
        // Work against the right edge, then reflect/swap for the selected side.
        let vertical = abs(tip.y / half_size.y) >= abs(tip.x / half_size.x);
        var local_tip = tip;
        var local_size = half_size;
        if vertical {
            local_tip = tip.yx;
            local_size = half_size.yx;
        }
        let half_width = min(p.tail_width * 0.5, local_size.y);
        let along = clamp(local_tip.y, -local_size.y + half_width, local_size.y - half_width);
        var start = side_point(along - half_width, local_size, radius);
        var end = side_point(along + half_width, local_size, radius);
        // Overlap the body so the triangle's base cannot leave an inner stroke.
        start.x = max(start.x - half_width, 0.0);
        end.x = max(end.x - half_width, 0.0);
        start.x *= sign(local_tip.x);
        end.x *= sign(local_tip.x);
        if vertical {
            start = start.yx;
            end = end.yx;
        }
        distance = min(distance, triangle_distance(point, start, end, tip));
    }
    return shape_color(distance, p.fill_enabled, p.color, p.outline_width, p.outline_position, p.outline_color);
}
