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

    pub(super) fn time_grid(&self, selected: &SelectedCurve, beat_guide: BeatGuide) -> CurveGrid {
        let frame_rate = selected.frame_rate;
        let start = f64::from(selected.start_seconds);
        let duration = f64::from(selected.duration_seconds).max(f64::EPSILON);
        let end = start + duration;
        let pixels_per_second = self.graph_pixels_per_second(selected.duration_seconds);
        let to_normalized = |seconds: f64| ((seconds - start) / duration) as f32;
        // Repeat labels use pattern time; guides retain their timeline origin.
        let timeline_start = selected
            .clock
            .time_at(selected.source_stop_positions[selected.source_segment])
            .seconds(frame_rate);
        let timeline_ticks = |grid: time_grid::Grid| {
            grid.visible_times(timeline_start, timeline_start + duration)
                .into_iter()
                .map(|seconds| ((seconds - timeline_start) / duration) as f32)
                .filter(|progress| (0. ..=1.).contains(progress))
                .collect()
        };

        CurveGrid {
            ruler_ticks: time_grid::Grid::seconds(pixels_per_second)
                .visible_times(start, end)
                .into_iter()
                .filter(|seconds| (start..=end).contains(seconds))
                .map(|seconds| (seconds, to_normalized(seconds)))
                .collect(),
            frame_ticks: timeline_ticks(time_grid::Grid::frames(pixels_per_second, frame_rate)),
            beat_ticks: timeline_ticks(time_grid::Grid::beats(
                pixels_per_second,
                beat_guide,
                frame_rate,
            )),
            values: Vec::new(),
        }
    }
}
