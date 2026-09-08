#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct MoveOrigin {
    pub start: u64,
    pub duration: u64,
    pub layer: u64,
}

pub(super) fn virtual_layer_count(
    highest_occupied_layer: Option<usize>,
    visible_rows: usize,
    minimum_rows: usize,
    trailing_rows: usize,
) -> usize {
    highest_occupied_layer
        .map_or(1, |layer| layer.saturating_add(2))
        .saturating_add(trailing_rows)
        .max(visible_rows.saturating_add(trailing_rows))
        .max(minimum_rows)
}

pub(super) fn clamp_move_delta(
    origins: &[MoveOrigin],
    frame_delta: i64,
    layer_delta: i64,
) -> (i64, i64) {
    let min_start = origins.iter().map(|item| item.start).min().unwrap_or(0);
    let max_end = origins
        .iter()
        .map(|item| item.start.saturating_add(item.duration))
        .max()
        .unwrap_or(0);
    let min_layer = origins.iter().map(|item| item.layer).min().unwrap_or(0);
    let max_layer = origins.iter().map(|item| item.layer).max().unwrap_or(0);

    let min_frame_delta = negative_bound(min_start);
    let max_frame_delta = positive_bound(u64::MAX.saturating_sub(max_end));
    let min_layer_delta = negative_bound(min_layer);
    let max_layer_delta = positive_bound(u64::MAX.saturating_sub(max_layer));

    (
        frame_delta.clamp(min_frame_delta, max_frame_delta),
        layer_delta.clamp(min_layer_delta, max_layer_delta),
    )
}

fn negative_bound(value: u64) -> i64 {
    if value > i64::MAX as u64 {
        i64::MIN
    } else {
        -(value as i64)
    }
}

fn positive_bound(value: u64) -> i64 {
    value.min(i64::MAX as u64) as i64
}

const SNAP_THRESHOLD_PIXELS: f64 = 8.;

pub(super) fn nearest_snap_offset(
    moving_times: &[f64],
    targets: &[f64],
    threshold_seconds: f64,
) -> f64 {
    moving_times
        .iter()
        .flat_map(|moving| targets.iter().map(move |target| target - moving))
        .filter(|offset| offset.abs() <= threshold_seconds)
        .min_by(|left, right| left.abs().total_cmp(&right.abs()))
        .unwrap_or(0.)
}

pub(super) fn snap_offset_seconds(
    moving_times: &[f64],
    explicit_targets: &[f64],
    major_step: f64,
    pixels_per_second: f64,
) -> f64 {
    if moving_times.is_empty()
        || !major_step.is_finite()
        || major_step <= 0.
        || !pixels_per_second.is_finite()
        || pixels_per_second <= 0.
    {
        return 0.;
    }

    let threshold = SNAP_THRESHOLD_PIXELS / pixels_per_second;
    let moving_times = moving_times
        .iter()
        .copied()
        .filter(|time| time.is_finite())
        .collect::<Vec<_>>();
    let explicit_targets = explicit_targets
        .iter()
        .copied()
        .filter(|time| time.is_finite())
        .collect::<Vec<_>>();
    let explicit = nearest_snap_offset(&moving_times, &explicit_targets, threshold);
    let grid_targets = moving_times
        .iter()
        .map(|time| (time / major_step).round() * major_step)
        .filter(|time| *time >= 0.)
        .collect::<Vec<_>>();
    let grid = nearest_snap_offset(&moving_times, &grid_targets, threshold);
    if explicit != 0. && (grid == 0. || explicit.abs() <= grid.abs()) {
        explicit
    } else {
        grid
    }
}
