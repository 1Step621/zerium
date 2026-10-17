import package::generated::support::util::DEG2RAD;
import package::generated::sector::props;
import package::helpers::geometry::{PI, TAU, cross_2d, segment_distance};
import package::helpers::shape::{ellipse_distance, outlined_quad, shape_color};

fn arc_derivative(point: vec2<f32>, axes: vec2<f32>, angle: f32) -> f32 {
    let sine = sin(angle);
    let cosine = cos(angle);
    return (axes.y * axes.y - axes.x * axes.x) * sine * cosine + axes.x * point.x * sine
        - axes.y * point.y * cosine;
}

fn quadrant_arc_distance(point: vec2<f32>, axes: vec2<f32>, low: f32, high: f32) -> f32 {
    var closest = min(
        distance(point, axes * vec2(cos(low), sin(low))),
        distance(point, axes * vec2(cos(high), sin(high))),
    );

    if abs(axes.x - axes.y) < 0.00001 {
        let angle = atan2(point.y, point.x);

        if angle >= low && angle <= high {
            closest = min(closest, distance(point, axes * vec2(cos(angle), sin(angle))));
        }

        return closest;
    }

    // On a quadrant there is at most one interior minimum. When the query is
    // across one of the axes, split at the derivative's extremum so bisection
    // cannot converge to the other stationary point (a maximum).
    var first = 0.0;
    var last = PI * 0.5;
    var has_minimum = point.x >= 0.0 && point.y >= 0.0;

    if point.x < 0.0 && point.y > 0.0 && axes.y > axes.x {
        last = atan(pow(axes.y * point.y / (-axes.x * point.x), 1.0 / 3.0));
        has_minimum = arc_derivative(point, axes, last) > 0.0;
    } else if point.x > 0.0 && point.y < 0.0 && axes.x > axes.y {
        first = atan(pow(-axes.y * point.y / (axes.x * point.x), 1.0 / 3.0));
        has_minimum = arc_derivative(point, axes, first) < 0.0;
    }

    if has_minimum {
        for(var iteration = 0u; iteration < 20u; iteration += 1u) {
            let angle = (first + last) * 0.5;

            if arc_derivative(point, axes, angle) < 0.0 {
                first = angle;
            } else {
                last = angle;
            }
        }

        let angle = (first + last) * 0.5;

        if angle >= low && angle <= high {
            closest = min(closest, distance(point, axes * vec2(cos(angle), sin(angle))));
        }
    }

    return closest;
}

fn ellipse_arc_distance(point: vec2<f32>, axes: vec2<f32>, start: f32, sweep: f32) -> f32 {
    let quarter = PI * 0.5;
    let first = u32(floor(start / quarter));
    let end = start + sweep;
    var closest = 1e20;

    for(var index = 0u; index < 5u; index += 1u) {
        let quadrant = first + index;
        let base = f32(quadrant) * quarter;
        var low = max(start - base, 0.0);
        var high = min(end - base, quarter);

        if low >= high {
            continue;
        }

        let section = quadrant % 4u;

        if section % 2u == 1u {
            let previous_low = low;
            low = quarter - high;
            high = quarter - previous_low;
        }

        let sign = vec2(
            select(1.0, -1.0, section == 1u || section == 2u),
            select(1.0, -1.0, section >= 2u),
        );
        closest = min(closest, quadrant_arc_distance(point * sign, axes, low, high));
    }

    return closest;
}

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) local: vec2<f32>,
    @interpolate(flat)
    @location(1) instance_index: u32,
}

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

    if properties.sweep_angle <= 0.0 {
        return vec4(0.0);
    }

    let half_size = vec2(properties.size.v0, properties.size.v1) * 0.5;
    let point = input.local * half_size;
    let radius = length(input.local);
    var distance = (radius - 1.0) * min(half_size.x, half_size.y);
    let inner_radius = properties.inner_radius / 100.0;

    if inner_radius > 0.0 {
        distance = max(distance, (inner_radius - radius) * min(half_size.x, half_size.y));
    }

    if properties.sweep_angle < 360.0 {
        let start = properties.start_angle * DEG2RAD;
        let end = (properties.start_angle + properties.sweep_angle) * DEG2RAD;
        let start_direction = vec2(cos(start), sin(start)) * half_size;
        let end_direction = vec2(cos(end), sin(end)) * half_size;
        let start_distance = -cross_2d(start_direction, point) / length(start_direction);
        let end_distance = cross_2d(end_direction, point) / length(end_direction);
        let wedge_distance = select(
            min(start_distance, end_distance),
            max(start_distance, end_distance),
            properties.sweep_angle <= 180.0,
        );
        distance = max(distance, wedge_distance);
    }

    if properties.outline_width > 0.0 {
        if properties.sweep_angle >= 360.0 {
            distance = ellipse_distance(point, half_size);

            if inner_radius > 0.0 {
                distance = max(distance, -ellipse_distance(point, half_size * inner_radius));
            }
        } else {
            let start = fract(properties.start_angle / 360.0) * TAU;
            let sweep = properties.sweep_angle * DEG2RAD;
            let end = start + sweep;
            let first = half_size * vec2(cos(start), sin(start));
            let last = half_size * vec2(cos(end), sin(end));
            var closest = ellipse_arc_distance(point, half_size, start, sweep);

            if inner_radius > 0.0 {
                closest = min(
                    closest,
                    ellipse_arc_distance(point, half_size * inner_radius, start, sweep),
                );
            }

            closest = min(closest, segment_distance(point, first * inner_radius, first));
            closest = min(closest, segment_distance(point, last * inner_radius, last));
            distance = select(closest, -closest, distance <= 0.0);
        }
    }

    return shape_color(
        distance,
        properties.fill_enabled,
        properties.color,
        properties.outline_width,
        properties.outline_position,
        properties.outline_color,
    );
}
