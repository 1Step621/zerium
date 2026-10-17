// Colors are supplied by the caller; this helper is independent of properties
// and color space. Stops need not be ordered, so animated positions may cross.
struct GradientSegment {
    start_position: f32,
    start_color: vec4<f32>,
    end_position: f32,
    end_color: vec4<f32>,
}

fn gradient_start(position: f32, color: vec4<f32>) -> GradientSegment {
    return GradientSegment(position, color, position, color);
}

fn gradient_include(
    segment: GradientSegment,
    position: f32,
    color: vec4<f32>,
    sample_position: f32,
) -> GradientSegment {
    var result = segment;

    if position <= sample_position {
        // At coincident stops, the last declared stop takes the right side.

        if result.start_position > sample_position || position >= result.start_position {
            result.start_position = position;
            result.start_color = color;
        }
    } else {
        // The first declared stop takes the left side of a hard transition.

        if result.end_position <= sample_position || position < result.end_position {
            result.end_position = position;
            result.end_color = color;
        }
    }

    return result;
}

fn gradient_factor(segment: GradientSegment, sample_position: f32) -> f32 {
    // Hold the nearest endpoint outside the range of the stops.

    if segment.start_position > sample_position {
        return 1.0;
    }

    if segment.end_position <= sample_position {
        return 0.0;
    }

    return clamp(
        (sample_position - segment.start_position)
        / (segment.end_position - segment.start_position),
        0.0,
        1.0,
    );
}
