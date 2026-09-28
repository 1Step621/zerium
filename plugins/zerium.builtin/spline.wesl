struct SplineCurve {
    b0: vec2<f32>,
    b1: vec2<f32>,
    b2: vec2<f32>,
    b3: vec2<f32>,
};

struct SplineIndices {
    p0: u32,
    p1: u32,
    p2: u32,
    p3: u32,
};

fn spline_segment_count(point_count: u32, closed: bool) -> u32 {
    if point_count < 2u {
        return 0u;
    }
    return select(point_count - 1u, point_count, closed && point_count >= 3u);
}

fn spline_indices(segment: u32, point_count: u32, closed: bool) -> SplineIndices {
    if closed && point_count >= 3u {
        return SplineIndices(
            (segment + point_count - 1u) % point_count,
            segment,
            (segment + 1u) % point_count,
            (segment + 2u) % point_count,
        );
    }
    return SplineIndices(
        max(segment, 1u) - 1u,
        segment,
        segment + 1u,
        min(segment + 2u, point_count - 1u),
    );
}

fn spline_curve(
    p0: vec2<f32>,
    p1: vec2<f32>,
    p2: vec2<f32>,
    p3: vec2<f32>,
    tension: f32,
) -> SplineCurve {
    let handle_scale = (1.0 - clamp(tension / 100.0, -1.0, 1.0)) / 6.0;
    let tangent1 = (p2 - p0) * handle_scale;
    let tangent2 = (p3 - p1) * handle_scale;
    return SplineCurve(
        p1,
        p1 + tangent1,
        p2 - tangent2,
        p2,
    );
}

fn spline_point(curve: SplineCurve, t: f32) -> vec2<f32> {
    let u = 1.0 - t;
    return u * u * u * curve.b0
        + 3.0 * u * u * t * curve.b1
        + 3.0 * u * t * t * curve.b2
        + t * t * t * curve.b3;
}

// Keep the chord approximation within roughly 1/8 composition pixel. The
// second derivative bounds the error of each straight subsegment.
fn spline_steps(curve: SplineCurve) -> u32 {
    let bend = max(
        length(6.0 * (curve.b2 - 2.0 * curve.b1 + curve.b0)),
        length(6.0 * (curve.b3 - 2.0 * curve.b2 + curve.b1)),
    );
    return u32(clamp(ceil(sqrt(bend)), 8.0, 256.0));
}

fn spline_length(curve: SplineCurve) -> f32 {
    let steps = spline_steps(curve);
    var previous = curve.b0;
    var result = 0.0;
    for (var step = 1u; step <= steps; step += 1u) {
        let current = spline_point(curve, f32(step) / f32(steps));
        result += distance(previous, current);
        previous = current;
    }
    return result;
}

fn segment_distance(point: vec2<f32>, start: vec2<f32>, end: vec2<f32>) -> f32 {
    let direction = end - start;
    let length_squared = dot(direction, direction);
    let fraction = clamp(dot(point - start, direction) / max(length_squared, 0.000001), 0.0, 1.0);
    return distance(point, start + fraction * direction);
}
