import package::generated::host::util::scene_color;
import package::generated::host::item::quad_position;
import package::helpers::geometry::distance_coverage;
import package::helpers::quad::{PlacedQuad, placed_quad};

fn outline_outset(width: f32, position: u32) -> f32 {
    // Match Outline: 0 = outside, 1 = center, 2 = inside.
    return max(width, 0.0) * (2.0 - f32(position)) * 0.5;
}

// Expand only the raster quad. The original size still controls the placement
// anchor and local coordinates, so changing stroke position never moves a shape.
fn outlined_quad(
    vertex_index: u32,
    instance_index: u32,
    position: vec2<f32>,
    size: vec2<f32>,
    origin: vec2<u32>,
    width: f32,
    outline_position: u32,
) -> PlacedQuad {
    let quad = placed_quad(vertex_index, instance_index, position, size, origin);
    let padded_size = size + vec2(2.0 * outline_outset(width, outline_position));
    return PlacedQuad(
        quad_position(instance_index, quad.center, quad.corner, padded_size),
        quad.corner * padded_size / size,
        quad.center,
    );
}

// Distances and stroke widths are in composition pixels. Composite the inside
// and outside coverage separately so translucent fills do not dim the stroke.
fn shape_color(
    distance: f32,
    fill_enabled: bool,
    fill_color: vec4<f32>,
    outline_width: f32,
    outline_position: u32,
    outline_color: vec4<f32>,
) -> vec4<f32> {
    let width = max(outline_width, 0.0);
    let outset = outline_outset(width, outline_position);
    let fill_coverage = distance_coverage(distance, 0.00001);
    let outer_coverage = distance_coverage(distance - outset, 0.00001);
    let inner_coverage = distance_coverage(distance - outset + width, 0.00001);
    let stroke_coverage = max(outer_coverage - inner_coverage, 0.0);
    let inside_coverage = max(fill_coverage - inner_coverage, 0.0);
    let fill = scene_color(fill_color) * select(0.0, 1.0, fill_enabled);
    let stroke = scene_color(outline_color);
    return stroke * stroke_coverage + fill * (fill_coverage - stroke.a * inside_coverage);
}

// Euclidean distance to an ellipse. Solve the closest-point multiplier rather
// than scaling a circle's distance, so an inset stroke keeps its pixel width
// on elongated ellipses as well, including the inner arc of an annular sector.
fn ellipse_distance(point: vec2<f32>, half_size: vec2<f32>) -> f32 {
    var p = abs(point);
    var axes = half_size;
    if axes.x < axes.y {
        axes = axes.yx;
        p = p.yx;
    }
    if abs(axes.x - axes.y) < 0.00001 {
        return length(p) - axes.x;
    }
    p /= axes.x;
    let ratio = axes.y / axes.x;
    let squared = vec2(1.0, ratio * ratio);
    let inside = dot(p / vec2(1.0, ratio), p / vec2(1.0, ratio)) <= 1.0;
    if p.y < 0.0000001 {
        let closest_x = min(p.x / (1.0 - squared.y), 1.0);
        let closest = vec2(closest_x, ratio * sqrt(max(1.0 - closest_x * closest_x, 0.0)));
        return distance(p, closest) * axes.x * select(1.0, -1.0, inside);
    }
    let in_quad = all(p <= vec2(1.0, ratio));
    var low = select(0.0, -squared.y, inside);
    var high = select(length(vec2(1.0, ratio) * p), ratio, in_quad);
    high = select(high, 0.0, inside);
    let iterations = select(32u, 20u, in_quad);
    for (var iteration = 0u; iteration < iterations; iteration += 1u) {
        let multiplier = (low + high) * 0.5;
        let normalized = vec2(1.0, ratio) * p / (squared + vec2(multiplier));
        if dot(normalized, normalized) > 1.0 {
            low = multiplier;
        } else {
            high = multiplier;
        }
    }
    let closest = squared * p / (squared + vec2((low + high) * 0.5));
    return distance(p, closest) * axes.x * select(1.0, -1.0, inside);
}
