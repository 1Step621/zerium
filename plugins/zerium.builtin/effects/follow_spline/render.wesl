import package::generated::host::render::vertex;
import package::generated::host::effect::{uv_to_position, input_position, sample_input};
import package::generated::follow_spline::{ZeriumProps, get_points, props};
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

fn path_progress(properties: ZeriumProps, progress: f32) -> vec2<f32> {
    if properties.points_len == 0u {
        return vec2(0.0);
    }

    if properties.points_len == 1u {
        return path_point(properties, 0u);
    }

    let count = spline_segment_count(properties.points_len, properties.closed);
    let total = path_length(properties);

    if total <= 0.000001 {
        return path_point(properties, 0u);
    }

    let travel_length = clamp(progress / 100.0, 0.0, 1.0) * total;
    var traversed = 0.0;

    for(var segment = 0u; segment < count; segment += 1u) {
        let curve = path_curve(properties, segment);
        let steps = spline_steps(curve);
        var previous = curve.b0;

        for(var step = 1u; step <= steps; step += 1u) {
            let current = spline_point(curve, f32(step) / f32(steps));
            let length = distance(previous, current);

            if traversed + length >= travel_length {
                let fraction = clamp((travel_length - traversed) / max(length, 0.000001), 0.0, 1.0);
                return mix(previous, current, fraction);
            }

            traversed += length;
            previous = current;
        }
    }

    return path_point(
        properties,
        select(
            properties.points_len - 1u,
            0u,
            properties.closed && properties.points_len >= 3u,
        ),
    );
}

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @interpolate(flat)
    @location(1) offset: vec2<f32>,
}

fn path_position(properties: ZeriumProps) -> vec2<f32> {
    let position = vec2(properties.position.v0, properties.position.v1);
    return position + path_progress(properties, properties.progress);
}

@vertex
fn vertex_main(@builtin(vertex_index) vertex_index: u32) -> VertexOutput {
    let base = vertex(vertex_index);
    var output: VertexOutput;
    output.position = base.position;
    output.uv = base.uv;
    output.offset = path_position(props()) - input_position(vec2(0.5));
    return output;
}

@fragment
fn fragment_main(input: VertexOutput) -> @location(0) vec4<f32> {
    return sample_input(uv_to_position(input.uv) - input.offset);
}
