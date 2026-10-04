use super::*;

impl AnimationCurveEditor {
    pub(super) fn normalized_value(value: f64, minimum: f64, maximum: f64) -> f32 {
        ((value - minimum) / (maximum - minimum)) as f32
    }

    pub(super) fn nice_value_step(range: f64) -> f64 {
        let rough_step = range.abs() / 5.;
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

    pub(super) fn value_grid(minimum: f64, maximum: f64) -> Vec<(f64, f32)> {
        let range = maximum - minimum;
        if range == 0. {
            return vec![(minimum, 0.5)];
        }
        let step = Self::nice_value_step(range);
        let first = (minimum / step).ceil() * step;
        let count = ((maximum - first) / step).floor().max(0.) as usize;
        let ticks = (0..=count.min(31))
            .filter_map(|index| {
                let value = first + index as f64 * step;
                (value.is_finite() && (minimum..=maximum).contains(&value))
                    .then(|| (value, Self::normalized_value(value, minimum, maximum)))
            })
            .collect::<Vec<_>>();
        if ticks.is_empty() {
            vec![(minimum, 0.), (maximum, 1.)]
        } else {
            ticks
        }
    }

    pub(super) fn time_grid(
        &self,
        start_seconds: f32,
        duration_seconds: f32,
        frame_rate: FrameRate,
    ) -> CurveGrid {
        let duration_seconds = duration_seconds.max(f32::EPSILON);
        let visible_start = start_seconds;
        let visible_end = start_seconds + duration_seconds;
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
