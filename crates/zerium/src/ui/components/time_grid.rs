use std::collections::HashSet;
use zerium_core::timeline::{BeatGuide, Frame, FrameRate, ItemId, TimelineEditor};

const TARGET_RULER_TICK_SPACING: f64 = 90.;
const TARGET_FRAME_GRID_SPACING: f64 = 10.;
const SNAP_THRESHOLD_PIXELS: f64 = 8.;

pub(crate) fn format_timestamp(seconds: f64) -> String {
    let total_seconds = seconds.round().clamp(0., u64::MAX as f64) as u64;
    let seconds = total_seconds % 60;
    let total_minutes = total_seconds / 60;
    let minutes = total_minutes % 60;
    let hours = total_minutes / 60;
    format!("{hours:02}:{minutes:02}:{seconds:02}")
}

/// Regular guide times, optionally rounded to frames for drawing and snapping.
#[derive(Clone, Copy)]
pub(crate) struct Grid {
    step_seconds: f64,
    origin_seconds: f64,
    frame_rate: Option<FrameRate>,
}

impl Grid {
    pub(crate) fn seconds(pixels_per_second: f64) -> Self {
        const STEPS: [f64; 11] = [1., 2., 5., 10., 20., 30., 60., 120., 300., 600., 1_800.];

        let desired_step = TARGET_RULER_TICK_SPACING / pixels_per_second.max(f64::EPSILON);
        Self {
            step_seconds: STEPS
                .into_iter()
                .find(|step| *step >= desired_step)
                .unwrap_or(3_600.),
            origin_seconds: 0.,
            frame_rate: None,
        }
    }

    pub(crate) fn frames(pixels_per_second: f64, frame_rate: FrameRate) -> Self {
        const STEPS: [u64; 13] = [1, 2, 3, 5, 10, 15, 30, 60, 120, 300, 600, 1_800, 3_600];

        let frame_seconds = 1. / frame_rate.frames_per_second();
        let pixels_per_frame = pixels_per_second * frame_seconds;
        let frames = STEPS
            .into_iter()
            .find(|frames| *frames as f64 * pixels_per_frame >= TARGET_FRAME_GRID_SPACING)
            .unwrap_or(3_600);
        Self {
            step_seconds: frames as f64 * frame_seconds,
            origin_seconds: 0.,
            frame_rate: Some(frame_rate),
        }
    }

    pub(crate) fn beats(pixels_per_second: f64, guide: BeatGuide, frame_rate: FrameRate) -> Self {
        let beat_seconds = guide.beat_seconds();
        Self {
            step_seconds: Self::seconds(pixels_per_second * beat_seconds).step_seconds
                * beat_seconds,
            origin_seconds: f64::from(guide.offset_seconds()),
            frame_rate: Some(frame_rate),
        }
    }

    pub(crate) fn visible_times(self, start: f64, end: f64) -> Vec<f64> {
        let first_index = ((start - self.origin_seconds) / self.step_seconds).floor();
        let count = (((end - self.origin_seconds) / self.step_seconds).ceil() - first_index).max(0.)
            as usize
            + 1;
        (0..count)
            .filter_map(|index| self.time_at(first_index + index as f64))
            .collect()
    }

    fn neighboring_times(self, seconds: f64) -> impl Iterator<Item = f64> {
        let index = ((seconds - self.origin_seconds) / self.step_seconds).floor();
        [index, index + 1.]
            .into_iter()
            .filter_map(move |index| self.time_at(index))
    }

    fn time_at(self, index: f64) -> Option<f64> {
        let seconds = self.origin_seconds + index * self.step_seconds;
        (seconds >= 0.).then(|| {
            self.frame_rate.map_or(seconds, |frame_rate| {
                frame_rate.frame_to_seconds(frame_rate.seconds_to_frame(seconds))
            })
        })
    }
}

/// Beats take priority within the snap threshold. Otherwise use item edges,
/// the gesture's starting playhead, and frame guides. Targets use editable frames.
pub(crate) fn snap_offset_seconds(
    moving_times: &[f64],
    editor: &TimelineEditor,
    snap_playhead: Frame,
    excluded_items: impl IntoIterator<Item = ItemId>,
    pixels_per_second: f64,
) -> f64 {
    if !pixels_per_second.is_finite() || pixels_per_second <= 0. {
        return 0.;
    }
    let frame_rate = editor.frame_rate();
    let threshold = SNAP_THRESHOLD_PIXELS / pixels_per_second;
    let nearest = |targets: &[f64], grid: Grid| {
        moving_times
            .iter()
            .copied()
            .filter(|time| time.is_finite())
            .flat_map(|moving| {
                let guides = grid.neighboring_times(moving);
                targets
                    .iter()
                    .copied()
                    .chain(guides)
                    .map(move |target| target - moving)
            })
            .filter(|offset| offset.abs() <= threshold)
            .min_by(|left, right| left.abs().total_cmp(&right.abs()))
    };
    nearest(
        &[],
        Grid::beats(pixels_per_second, editor.beat_guide(), frame_rate),
    )
    .or_else(|| {
        let excluded_items = excluded_items.into_iter().collect::<HashSet<_>>();
        let targets = std::iter::once(snap_playhead)
            .chain(
                editor
                    .item_time_ranges()
                    .filter(|(item_id, _, _)| !excluded_items.contains(item_id))
                    .flat_map(|(_, start, end)| [start, end]),
            )
            .map(|frame| frame_rate.frame_to_seconds(frame))
            .collect::<Vec<_>>();
        nearest(&targets, Grid::frames(pixels_per_second, frame_rate))
    })
    .unwrap_or(0.)
}
