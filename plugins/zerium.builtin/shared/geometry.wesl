const PI: f32 = 3.141592653589793;
const TAU: f32 = 6.28318530718;

fn cross_2d(a: vec2<f32>, b: vec2<f32>) -> f32 {
    return a.x * b.y - a.y * b.x;
}

// Even-odd polygon fill: count intersections with the horizontal ray from point.
fn crosses_ray(point: vec2<f32>, previous: vec2<f32>, current: vec2<f32>) -> bool {
    if (current.y > point.y) != (previous.y > point.y) {
        let edge_x = (previous.x - current.x)
            * (point.y - current.y)
            / (previous.y - current.y)
            + current.x;
        return point.x < edge_x;
    }
    return false;
}

fn segment_distance(point: vec2<f32>, start: vec2<f32>, end: vec2<f32>) -> f32 {
    let direction = end - start;
    let length_squared = dot(direction, direction);
    let fraction = clamp(dot(point - start, direction) / max(length_squared, 0.000001), 0.0, 1.0);
    return distance(point, start + fraction * direction);
}

fn distance_coverage(distance: f32, minimum_width: f32) -> f32 {
    let width = max(fwidth(distance), minimum_width);
    return 1.0 - smoothstep(-width * 0.5, width * 0.5, distance);
}
