//! Collision-aware insertion, movement, and resizing of timeline intervals.

use std::{collections::HashSet, sync::Arc};

use crate::timeline::{Frame, FrameDuration, FrameRate, ItemId, LayerId, TimelineItem};

use super::TimelineDocument;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResizeEdge {
    Left,
    Right,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ResizeMode {
    #[default]
    Trim,
    Stretch,
}

impl ResizeEdge {
    pub(in crate::timeline) fn item_frame(self, item: &TimelineItem) -> u64 {
        match self {
            Self::Left => item.start.get(),
            Self::Right => item.end_exclusive().get(),
        }
    }

    fn delta_bounds(
        self,
        item: &TimelineItem,
        previous_end: u64,
        next_start: u64,
        frame_rate: FrameRate,
        mode: ResizeMode,
    ) -> (i128, i128) {
        let start = item.start.get();
        let end = item.end_exclusive().get();
        let (mut lower, mut upper) = match self {
            Self::Left => (
                i128::from(previous_end) - i128::from(start),
                i128::from(end.saturating_sub(1)) - i128::from(start),
            ),
            Self::Right => (
                i128::from(start.saturating_add(1)) - i128::from(end),
                i128::from(next_start) - i128::from(end),
            ),
        };
        if let Some(mapping) = item.timeline_mapping() {
            match mode {
                ResizeMode::Trim => match self {
                    Self::Left => {
                        let available = i128::from(mapping.extend_left_frames(frame_rate));
                        lower = lower.max(-available);
                    }
                    Self::Right => {}
                },
                ResizeMode::Stretch => {
                    let (min_speed, max_speed) = item.timeline_speed_bounds();
                    let minimum = i128::from(
                        mapping
                            .with_speed(max_speed)
                            .expect("validated speed bounds")
                            .timeline_duration(frame_rate)
                            .get(),
                    );
                    let maximum = i128::from(
                        mapping
                            .with_speed(min_speed)
                            .expect("validated speed bounds")
                            .timeline_duration(frame_rate)
                            .get(),
                    );
                    let duration = i128::from(item.duration.get());
                    let (min_delta, max_delta) = match self {
                        Self::Left => (duration - maximum, duration - minimum),
                        Self::Right => (minimum - duration, maximum - duration),
                    };
                    lower = lower.max(min_delta);
                    upper = upper.min(max_delta);
                }
            }
        }
        (lower, upper)
    }

    fn resize_by(
        self,
        item: &mut TimelineItem,
        delta: i128,
        frame_rate: FrameRate,
        mode: ResizeMode,
    ) -> Option<()> {
        if delta == 0 {
            return Some(());
        }
        let start = item.start.get();
        let end = item.end_exclusive().get();
        let (new_start, new_end) = match self {
            Self::Left => ((i128::from(start) + delta) as u64, end),
            Self::Right => (start, (i128::from(end) + delta) as u64),
        };
        let duration = FrameDuration::new_saturating(new_end - new_start);
        item.resize_to(Frame(new_start), duration, mode, frame_rate)
    }
}

impl TimelineDocument {
    pub(super) fn nearest_available_start(
        &self,
        layer: LayerId,
        requested: Frame,
        duration: FrameDuration,
        excluded_item: Option<ItemId>,
    ) -> Option<Frame> {
        let duration = duration.get();
        let max_start = u64::MAX.saturating_sub(duration);
        let requested = requested.get().min(max_start);
        let mut intervals = self
            .layer_items
            .get(&layer)
            .into_iter()
            .flatten()
            .filter(|id| Some(**id) != excluded_item)
            .filter_map(|id| self.items.get(id))
            .map(|item| (item.start.get(), item.end_exclusive().get()))
            .collect::<Vec<_>>();
        intervals.sort_unstable_by_key(|&(start, end)| (start, end));

        let mut cursor = 0;
        let mut best = None;
        let mut consider_gap = |first_start: u64, last_start: u64| {
            let candidate = requested.clamp(first_start, last_start);
            if best.is_none_or(|current: u64| {
                candidate.abs_diff(requested) < current.abs_diff(requested)
                    || (candidate.abs_diff(requested) == current.abs_diff(requested)
                        && candidate < current)
            }) {
                best = Some(candidate);
            }
        };

        for (start, end) in intervals {
            if let Some(last_start) = start.checked_sub(duration)
                && cursor <= last_start
            {
                consider_gap(cursor, last_start);
            }
            cursor = cursor.max(end);
        }
        if cursor <= max_start {
            consider_gap(cursor, max_start);
        }

        best.map(Frame::new)
    }

    pub(in crate::timeline) fn insert_item_copies(
        &mut self,
        sources: &[(LayerId, TimelineItem)],
        target_layer: LayerId,
        target_start: Frame,
    ) -> Option<Vec<(ItemId, ItemId)>> {
        let minimum_layer = sources.iter().map(|(layer, _)| layer.get()).min()?;
        let minimum_start = sources.iter().map(|(_, item)| item.start.get()).min()?;
        let mut copies = sources
            .iter()
            .map(|(layer, item)| {
                let layer_offset = layer.get().checked_sub(minimum_layer)?;
                let start_offset = item.start.get().checked_sub(minimum_start)?;
                let layer = LayerId::new(target_layer.get().checked_add(layer_offset)?);
                Some((layer, start_offset, item.clone()))
            })
            .collect::<Option<Vec<_>>>()?;
        copies.sort_unstable_by_key(|(layer, start_offset, item)| {
            (layer.get(), *start_offset, item.id.get())
        });

        let mut group_start = target_start.get();
        loop {
            let mut next_group_start = group_start;
            for (layer, start_offset, item) in &copies {
                let start = group_start.checked_add(*start_offset)?;
                let end = start.checked_add(item.duration.get())?;
                for existing in self
                    .layer_items
                    .get(layer)
                    .into_iter()
                    .flatten()
                    .filter_map(|id| self.items.get(id))
                {
                    if start < existing.end_exclusive().get() && existing.start.get() < end {
                        let required = existing.end_exclusive().get().checked_sub(*start_offset)?;
                        next_group_start = next_group_start.max(required);
                    }
                }
            }
            if next_group_start == group_start {
                break;
            }
            group_start = next_group_start;
        }

        let first_id = self.next_item_id?;
        let copy_count = u64::try_from(copies.len()).ok()?;
        let next_item_id = first_id
            .checked_add(copy_count)
            .filter(|id| *id != u64::MAX);
        let last_id = first_id.checked_add(copy_count.checked_sub(1)?)?;
        if last_id == u64::MAX {
            return None;
        }

        let mut id_map = Vec::with_capacity(copies.len());
        let mut affected_layers = HashSet::new();
        for (index, (layer, start_offset, mut item)) in copies.into_iter().enumerate() {
            let old_id = item.id;
            let index = u64::try_from(index).expect("copy count was already converted to u64");
            let new_id = ItemId(
                first_id
                    .checked_add(index)
                    .expect("last copied item ID was already checked"),
            );
            item.id = new_id;
            item.start = Frame::new(
                group_start
                    .checked_add(start_offset)
                    .expect("copied item end was already checked"),
            );
            self.items.insert(new_id, Arc::new(item));
            self.item_layers.insert(new_id, layer);
            self.layer_items.entry(layer).or_default().push(new_id);
            affected_layers.insert(layer);
            id_map.push((old_id, new_id));
        }
        for layer in affected_layers {
            if let Some(ids) = self.layer_items.get_mut(&layer) {
                ids.sort_unstable_by_key(|id| {
                    self.items
                        .get(id)
                        .map_or((Frame::new(0), id.get()), |item| (item.start, id.get()))
                });
            }
        }
        self.next_item_id = next_item_id;
        #[cfg(debug_assertions)]
        self.assert_consistent();
        Some(id_map)
    }

    pub fn resize_items_from(
        &mut self,
        origins: &[TimelineItem],
        anchor_id: ItemId,
        edge: ResizeEdge,
        pointer: Frame,
        mode: ResizeMode,
    ) -> bool {
        if origins.is_empty() {
            return false;
        }

        let moving = origins.iter().map(|item| item.id).collect::<HashSet<_>>();
        if moving.len() != origins.len()
            || !moving
                .iter()
                .all(|id| self.items.contains_key(id) && self.item_layers.contains_key(id))
        {
            return false;
        }
        let Some(anchor) = origins.iter().find(|item| item.id == anchor_id) else {
            return false;
        };
        let anchor_edge = edge.item_frame(anchor);
        let requested_delta = i128::from(pointer.get()) - i128::from(anchor_edge);
        let mut minimum_delta = i128::MIN;
        let mut maximum_delta = i128::MAX;

        for origin in origins {
            let layer = self.item_layers[&origin.id];
            let start = origin.start.get();
            let end = origin.end_exclusive().get();
            let mut previous_end = 0;
            let mut next_start = u64::MAX;
            for candidate in self
                .layer_items
                .get(&layer)
                .into_iter()
                .flatten()
                .filter_map(|candidate| self.items.get(candidate))
            {
                if candidate.end_exclusive().get() <= start {
                    previous_end = previous_end.max(candidate.end_exclusive().get());
                }
                if candidate.start.get() >= end {
                    next_start = next_start.min(candidate.start.get());
                }
            }

            let (lower, upper) =
                edge.delta_bounds(origin, previous_end, next_start, self.frame_rate, mode);
            minimum_delta = minimum_delta.max(lower);
            maximum_delta = maximum_delta.min(upper);
        }

        if minimum_delta > maximum_delta {
            return false;
        }
        let delta = requested_delta.clamp(minimum_delta, maximum_delta);
        let mut changed = false;

        let updates = origins
            .iter()
            .map(|origin| {
                let mut item = origin.clone();
                edge.resize_by(&mut item, delta, self.frame_rate, mode)?;
                item.validate_playback().ok()?;
                Some(item)
            })
            .collect::<Option<Vec<_>>>();
        let Some(updates) = updates else {
            return false;
        };
        for item in updates {
            if self.items[&item.id].as_ref() != &item {
                self.items.insert(item.id, Arc::new(item));
                changed = true;
            }
        }
        #[cfg(debug_assertions)]
        self.assert_consistent();
        changed
    }

    pub fn move_items_from(
        &mut self,
        origins: &[(ItemId, Frame, LayerId)],
        frame_delta: i64,
        layer_delta: i64,
    ) -> bool {
        if origins.is_empty() {
            return false;
        }

        let moving = origins.iter().map(|(id, _, _)| *id).collect::<HashSet<_>>();
        if moving.len() != origins.len() {
            return false;
        }

        let mut targets = Vec::with_capacity(origins.len());
        for (id, start, layer) in origins {
            let Some(item) = self.items.get(id) else {
                return false;
            };
            let Some(target_start) = start.get().checked_add_signed(frame_delta) else {
                return false;
            };
            let Some(target_layer) = layer.get().checked_add_signed(layer_delta) else {
                return false;
            };
            let Some(target_end) = target_start.checked_add(item.duration.get()) else {
                return false;
            };
            targets.push((*id, target_start, target_end, LayerId::new(target_layer)));
        }

        for (index, (_, start, end, layer)) in targets.iter().enumerate() {
            let collides_with_moving_item =
                targets
                    .iter()
                    .skip(index + 1)
                    .any(|(_, other_start, other_end, other_layer)| {
                        layer == other_layer && *start < *other_end && *other_start < *end
                    });
            if collides_with_moving_item {
                return false;
            }
            let collides_with_stationary_item = self
                .layer_items
                .get(layer)
                .into_iter()
                .flatten()
                .filter(|candidate| !moving.contains(candidate))
                .filter_map(|candidate| self.items.get(candidate))
                .any(|candidate| {
                    *start < candidate.end_exclusive().get() && candidate.start.get() < *end
                });
            if collides_with_stationary_item {
                return false;
            }
        }

        let changed = targets.iter().any(|(id, start, _, layer)| {
            self.items
                .get(id)
                .is_some_and(|item| item.start.get() != *start)
                || self.item_layers.get(id) != Some(layer)
        });
        if !changed {
            return false;
        }

        for ids in self.layer_items.values_mut() {
            ids.retain(|id| !moving.contains(id));
        }
        self.layer_items.retain(|_, ids| !ids.is_empty());
        for (id, start, _, layer) in targets {
            if let Some(item) = self.items.get_mut(&id).map(Arc::make_mut) {
                item.start = Frame::new(start);
            }
            self.item_layers.insert(id, layer);
            self.layer_items.entry(layer).or_default().push(id);
        }
        #[cfg(debug_assertions)]
        self.assert_consistent();
        true
    }
}
