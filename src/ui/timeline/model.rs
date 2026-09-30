pub(super) fn virtual_layer_count(
    highest_occupied_layer: Option<usize>,
    viewport_end: usize,
) -> usize {
    highest_occupied_layer
        .map_or(0, |layer| layer.saturating_add(1))
        .max(viewport_end)
        .saturating_add(1)
}

pub(super) fn clamp_move_delta(
    origins: impl Iterator<Item = (u64, u64, u64)>,
    frame_delta: i64,
    layer_delta: i64,
) -> (i64, i64) {
    let (min_start, max_end, min_layer, max_layer) = origins
        .map(|(start, duration, layer)| (start, start.saturating_add(duration), layer, layer))
        .reduce(|a, b| (a.0.min(b.0), a.1.max(b.1), a.2.min(b.2), a.3.max(b.3)))
        .unwrap_or((0, 0, 0, 0));
    (
        frame_delta.clamp(
            negative_bound(min_start),
            positive_bound(u64::MAX.saturating_sub(max_end)),
        ),
        layer_delta.clamp(
            negative_bound(min_layer),
            positive_bound(u64::MAX.saturating_sub(max_layer)),
        ),
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
