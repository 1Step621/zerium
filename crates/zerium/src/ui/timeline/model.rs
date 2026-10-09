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
