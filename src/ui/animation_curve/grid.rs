use super::*;

impl AnimationCurveEditor {
    pub(super) fn nice_value_step(range: f64) -> f64 {
        let rough_step = range.abs() / 5.;
        if !rough_step.is_finite() || rough_step <= f64::EPSILON {
            return 1.;
        }
        let magnitude = 10_f64.powf(rough_step.log10().floor());
        let fraction = rough_step / magnitude;
        let nice_fraction = if fraction <= 1. {
            1.
        } else if fraction <= 2. {
            2.
        } else if fraction <= 5. {
            5.
        } else {
            10.
        };
        nice_fraction * magnitude
    }

    pub(super) fn value_grid(animation: &GraphAnimation) -> Vec<(f64, f32)> {
        let minimum = animation.from.min(animation.to);
        let maximum = animation.from.max(animation.to);
        let range = maximum - minimum;
        if !range.is_finite() || range <= f64::EPSILON {
            return vec![(animation.from, 0.5)];
        }

        let step = Self::nice_value_step(range);
        let first = (minimum / step).ceil() * step;
        let count = ((maximum - first) / step).floor().max(0.) as usize + 1;
        (0..count.min(32))
            .map(|index| {
                let value = first + index as f64 * step;
                let normalized = (value - animation.from) / (animation.to - animation.from);
                (value, normalized as f32)
            })
            .collect()
    }

    pub(super) fn time_grid(
        &self,
        start_seconds: f32,
        duration_seconds: f32,
        frame_rate: FrameRate,
    ) -> CurveGrid {
        let duration_seconds = duration_seconds.max(f32::EPSILON);
        let visible_start = start_seconds + self.viewport.x_min * duration_seconds;
        let visible_end = start_seconds + self.viewport.x_max * duration_seconds;
        let pixels_per_second = self.graph_pixels_per_second(duration_seconds);
        let major_step = time_grid::ruler_step(pixels_per_second);
        let frame_step = time_grid::frame_grid_step(pixels_per_second, frame_rate);
        let item_end = start_seconds + duration_seconds;
        let is_visible = |seconds: f64| {
            (f64::from(visible_start)..=f64::from(visible_end)).contains(&seconds)
                && (f64::from(start_seconds)..=f64::from(item_end)).contains(&seconds)
        };
        let to_normalized = |seconds: f64| {
            ((seconds - f64::from(start_seconds)) / f64::from(duration_seconds)) as f32
        };

        CurveGrid {
            major: time_grid::visible_seconds(
                f64::from(visible_start),
                f64::from(visible_end),
                major_step,
            )
            .into_iter()
            .filter(|seconds| is_visible(*seconds))
            .map(|seconds| (seconds, to_normalized(seconds)))
            .collect(),
            minor: time_grid::visible_frames(
                f64::from(visible_start),
                f64::from(visible_end),
                frame_rate,
                frame_step,
            )
            .into_iter()
            .map(|frame| frame_rate.frame_to_seconds(frame))
            .filter(|seconds| is_visible(*seconds))
            .map(to_normalized)
            .collect(),
            values: Vec::new(),
        }
    }
}
