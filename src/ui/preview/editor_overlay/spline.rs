// Match the spline segment and chord sampling used by the built-in shaders.
pub(super) fn sample(points: &[[f32; 2]], tension: f32, closed: bool) -> Vec<[f32; 2]> {
    if points.len() < 2 {
        return Vec::new();
    }
    let closed = closed && points.len() >= 3;
    let segment_count = if closed {
        points.len()
    } else {
        points.len() - 1
    };
    let handle_scale = (1. - (tension / 100.).clamp(-1., 1.)) / 6.;
    let mut sampled = Vec::new();
    for segment in 0..segment_count {
        let count = points.len();
        let (p0, p1, p2, p3) = if closed {
            (
                points[(segment + count - 1) % count],
                points[segment],
                points[(segment + 1) % count],
                points[(segment + 2) % count],
            )
        } else {
            (
                points[segment.saturating_sub(1)],
                points[segment],
                points[segment + 1],
                points[(segment + 2).min(count - 1)],
            )
        };
        let b0 = p1;
        let b1 = add(p1, scale(sub(p2, p0), handle_scale));
        let b2 = sub(p2, scale(sub(p3, p1), handle_scale));
        let b3 = p2;
        let second_at_start = scale(add(sub(b2, scale(b1, 2.)), b0), 6.);
        let second_at_end = scale(add(sub(b3, scale(b2, 2.)), b1), 6.);
        let bend = length(second_at_start).max(length(second_at_end));
        let steps = bend.sqrt().ceil().clamp(8., 256.) as usize;
        if segment == 0 {
            sampled.push(b0);
        }
        for step in 1..=steps {
            let t = step as f32 / steps as f32;
            let u = 1. - t;
            sampled.push(add(
                add(scale(b0, u * u * u), scale(b1, 3. * u * u * t)),
                add(scale(b2, 3. * u * t * t), scale(b3, t * t * t)),
            ));
        }
    }
    sampled
}

fn add(a: [f32; 2], b: [f32; 2]) -> [f32; 2] {
    [a[0] + b[0], a[1] + b[1]]
}

fn sub(a: [f32; 2], b: [f32; 2]) -> [f32; 2] {
    [a[0] - b[0], a[1] - b[1]]
}

fn scale(a: [f32; 2], amount: f32) -> [f32; 2] {
    [a[0] * amount, a[1] * amount]
}

fn length(a: [f32; 2]) -> f32 {
    a[0].hypot(a[1])
}
