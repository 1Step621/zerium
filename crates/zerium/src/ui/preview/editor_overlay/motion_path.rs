//! Sample complete playback intervals so reducing path density cannot erase
//! short-period motion or connect distant occurrences with invented lines.
use super::*;

const MAX_PATH_WINDOWS: usize = 64;
const SAMPLES_PER_SEGMENT: usize = 32;

impl Preview {
    pub(super) fn motion_paths(
        item: &TimelineItem,
        effect_id: Option<EffectInstanceId>,
        property: &PropertySchema,
    ) -> Vec<Vec<[f32; 2]>> {
        let tracks = (0..2)
            .filter_map(|axis| item.animation_track(effect_id, property.id(), None, Some(axis)))
            .map(|track| (item.animation_clock(track), track))
            .collect::<Vec<_>>();
        let span = item.duration.get().saturating_sub(1) as f64;
        let window_span = tracks
            .iter()
            .map(|(clock, _)| clock.span_frames())
            .fold(item.animation_span_frames(), f64::min);
        let windows = (span / window_span).ceil() as usize;
        let stride = windows.div_ceil(MAX_PATH_WINDOWS).max(1);
        let mut indices = (0..windows).step_by(stride).collect::<Vec<_>>();
        if indices.last().copied() != windows.checked_sub(1)
            && let Some(last) = windows.checked_sub(1)
        {
            indices.push(last);
        }
        let mut paths = Vec::new();
        for index in indices {
            let start =
                TimelineTime::from_frames(item.start.get() as f64 + index as f64 * window_span);
            let end = TimelineTime::from_frames(
                item.start.get() as f64 + ((index + 1) as f64 * window_span).min(span),
            );
            let mut boundaries = vec![start, end];
            for (clock, track) in &tracks {
                for stop in track.stops() {
                    boundaries.extend(clock.occurrences(stop.position(), start, end));
                }
            }
            boundaries.sort_by(|left, right| left.frames().total_cmp(&right.frames()));
            boundaries.dedup();
            // Sample each interval without its right endpoint, then append the
            // final endpoint once. Shared boundaries never need deduplication.
            let path = boundaries
                .windows(2)
                .flat_map(|interval| {
                    let [start, end] = [interval[0], interval[1]];
                    (0..SAMPLES_PER_SEGMENT).map(move |sample| {
                        start.offset(
                            (end.frames() - start.frames()) * sample as f64
                                / SAMPLES_PER_SEGMENT as f64,
                        )
                    })
                })
                .chain(boundaries.last().copied())
                .filter_map(|time| Self::position_at_time(item, effect_id, property, time))
                .collect::<Vec<_>>();
            if !path.is_empty() {
                paths.push(path);
            }
        }
        paths
    }
}
