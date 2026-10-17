import package::generated::host::item::quad_position;
import package::generated::support::util::scene_color;
import package::generated::spline_path::{ZeriumProps, get_points, props};
import package::helpers::geometry::{crosses_ray, segment_distance};
import package::helpers::quad::placed_quad;
import package::helpers::spline::{
    SplineCurve,
    spline_curve,
    spline_indices,
    spline_length,
    spline_local_point,
    spline_point,
    spline_segment_count,
    spline_steps
};

fn path_point(properties: ZeriumProps, index: u32) -> vec2<f32> {
    let point = get_points(properties, index);
    let size = vec2(properties.size.v0, properties.size.v1);
    return spline_local_point(vec2(point.v0, point.v1), size);
}

fn path_curve(properties: ZeriumProps, segment: u32) -> SplineCurve {
    let indices = spline_indices(segment, properties.points_len, properties.closed);
    return spline_curve(
        path_point(properties, indices.p0),
        path_point(properties, indices.p1),
        path_point(properties, indices.p2),
        path_point(properties, indices.p3),
        properties.tension,
    );
}

fn path_length(properties: ZeriumProps) -> f32 {
    let count = spline_segment_count(properties.points_len, properties.closed);
    var total = 0.0;

    for(var segment = 0u; segment < count; segment += 1u) {
        total += spline_length(path_curve(properties, segment));
    }

    return total;
}

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) local: vec2<f32>,
    @interpolate(flat)
    @location(1) instance_index: u32,
    @interpolate(flat)
    @location(2) total_length: f32,
}

@vertex
fn vertex_main(
    @builtin(vertex_index) vertex_index: u32,
    @builtin(instance_index) instance_index: u32,
) -> VertexOutput {
    let properties = props(instance_index);
    let size = vec2(properties.size.v0, properties.size.v1);
    let quad = placed_quad(
        vertex_index,
        vec2(properties.position.v0, properties.position.v1),
        size,
        vec2(properties.origin.v0, properties.origin.v1),
    );

    // At tension -100, the curve can exceed the point quad by at most 1/4
    // of its size per axis. Allow a small margin for floating-point rounding.
    let padding = size * 0.251 + vec2(max(properties.width, 0.0));
    var output: VertexOutput;
    output.position = quad_position(quad.center, quad.corner, size + 2.0 * padding);
    output.local = quad.corner * (size * 0.5 + padding);
    output.instance_index = instance_index;
    output.total_length = 0.0;

    if properties.width > 0.0 && properties.stroke_start < properties.stroke_end
        && (properties.stroke_start > 0.0 || properties.stroke_end < 100.0) {
        output.total_length = path_length(properties);
    }

    return output;
}

@fragment
fn fragment_main(input: VertexOutput) -> @location(0) vec4<f32> {
    let properties = props(input.instance_index);
    let count = spline_segment_count(properties.points_len, properties.closed);
    let fill_enabled = properties.closed && properties.fill_enabled && properties.points_len >= 3u;
    let stroke_enabled = properties.width > 0.0 && properties.stroke_start < properties.stroke_end;

    if count == 0u || (!fill_enabled && !stroke_enabled) {
        discard;
    }

    let antialias = max(0.5 * length(fwidth(input.local)), 0.001);
    let trimmed = stroke_enabled
        && (properties.stroke_start > 0.0 || properties.stroke_end < 100.0);
    var trace_stroke = stroke_enabled;

    if trimmed {
        trace_stroke = false;

        for(var segment = 0u; segment < count; segment += 1u) {
            let curve = path_curve(properties, segment);
            let low = min(min(curve.b0, curve.b1), min(curve.b2, curve.b3));
            let high = max(max(curve.b0, curve.b1), max(curve.b2, curve.b3));
            let radius = vec2(properties.width * 0.5 + antialias);

            if all(input.local >= low - radius) && all(input.local <= high + radius) {
                trace_stroke = true;
                break;
            }
        }
    }

    if !fill_enabled && !trace_stroke {
        discard;
    }

    let trace_trim = trimmed && trace_stroke;
    let stroke_start = input.total_length * clamp(properties.stroke_start / 100.0, 0.0, 1.0);
    let stroke_end = input.total_length * clamp(properties.stroke_end / 100.0, 0.0, 1.0);
    var traversed = 0.0;
    var inside = false;
    var closest_fill = 1e20;
    var closest_stroke = 1e20;

    for(var segment = 0u; segment < count; segment += 1u) {
        let curve = path_curve(properties, segment);
        let curve_low = min(min(curve.b0, curve.b1), min(curve.b2, curve.b3));
        let curve_high = max(max(curve.b0, curve.b1), max(curve.b2, curve.b3));
        let radius = properties.width * 0.5 + antialias;
        let near_stroke = trace_stroke && all(input.local >= curve_low - vec2(radius))
            && all(input.local <= curve_high + vec2(radius));
        let relevant_fill =
            fill_enabled && input.local.y >= curve_low.y - antialias
            && input.local.y <= curve_high.y + antialias
            && input.local.x <= curve_high.x + antialias;

        if !relevant_fill && !trace_trim && !near_stroke {
            continue;
        }

        let steps = spline_steps(curve);
        var previous = curve.b0;

        for(var step = 1u; step <= steps; step += 1u) {
            let current = spline_point(curve, f32(step) / f32(steps));

            if relevant_fill {
                if crosses_ray(input.local, previous, current) {
                    inside = !inside;
                }

                if input.local.x >= curve_low.x - antialias {
                    closest_fill = min(
                        closest_fill,
                        segment_distance(input.local, previous, current),
                    );
                }
            }

            if trace_trim {
                let chord_length = distance(previous, current);

                if near_stroke && chord_length > 0.000001 {
                    let start_fraction = clamp((stroke_start - traversed) / chord_length, 0.0, 1.0);
                    let end_fraction = clamp((stroke_end - traversed) / chord_length, 0.0, 1.0);

                    if end_fraction > start_fraction {
                        closest_stroke = min(
                            closest_stroke,
                            segment_distance(
                                input.local,
                                mix(previous, current, start_fraction),
                                mix(previous, current, end_fraction),
                            ),
                        );
                    }
                }

                traversed += chord_length;
            } else if near_stroke {
                closest_stroke = min(
                    closest_stroke,
                    segment_distance(input.local, previous, current),
                );
            }

            previous = current;
        }
    }

    let stroke_coverage =
        clamp((properties.width * 0.5 + antialias - closest_stroke) / (2.0 * antialias), 0.0, 1.0);
    let fill_coverage = select(
        0.0,
        clamp(0.5 + select(-closest_fill, closest_fill, inside) / antialias, 0.0, 1.0),
        fill_enabled,
    );
    let stroke = scene_color(properties.color) * stroke_coverage;
    let fill = scene_color(properties.fill_color) * fill_coverage;
    return stroke + fill * (1.0 - stroke.a);
}
