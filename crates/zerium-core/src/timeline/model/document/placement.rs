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

#[derive(Clone, Copy)]
pub(in crate::timeline) enum CopyPlacement {
    Exact,
    FindSpace,
}

impl ResizeEdge {
    pub(in crate::timeline) fn item_frame(self, item: &TimelineItem, frame_rate: FrameRate) -> u64 {
        match self {
            Self::Left => item.start().get(),
            Self::Right => item.end_exclusive(frame_rate).get(),
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
        let start = item.start().get();
        let end = item.end_exclusive(frame_rate).get();
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
        match mode {
            ResizeMode::Trim => {
                if self == Self::Left
                    && let Some(available) = item.left_extension(frame_rate)
                {
                    lower = lower.max(-i128::from(available));
                }
            }
            ResizeMode::Stretch => {
                if let Some((minimum, maximum)) = item.stretch_duration_bounds(frame_rate) {
                    let minimum = i128::from(minimum);
                    let maximum = i128::from(maximum);
                    let duration = i128::from(item.duration(frame_rate).get());
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
        let start = item.start().get();
        let end = item.end_exclusive(frame_rate).get();
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
            .map(|item| {
                (
                    item.start().get(),
                    item.end_exclusive(self.frame_rate).get(),
                )
            })
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
        placement: CopyPlacement,
    ) -> Option<Vec<(ItemId, ItemId)>> {
        let minimum_layer = sources.iter().map(|(layer, _)| layer.get()).min()?;
        let minimum_start = sources.iter().map(|(_, item)| item.start().get()).min()?;
        let mut copies = sources
            .iter()
            .map(|(layer, item)| {
                let layer_offset = layer.get().checked_sub(minimum_layer)?;
                let start_offset = item.start().get().checked_sub(minimum_start)?;
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
                let end = start.checked_add(item.duration(self.frame_rate).get())?;
                for existing in self
                    .layer_items
                    .get(layer)
                    .into_iter()
                    .flatten()
                    .filter_map(|id| self.items.get(id))
                {
                    if start < existing.end_exclusive(self.frame_rate).get()
                        && existing.start().get() < end
                    {
                        let required = existing
                            .end_exclusive(self.frame_rate)
                            .get()
                            .checked_sub(*start_offset)?;
                        next_group_start = next_group_start.max(required);
                    }
                }
            }
            if next_group_start == group_start {
                break;
            }
            if let CopyPlacement::Exact = placement {
                return None;
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
            item.move_to(Frame::new(
                group_start
                    .checked_add(start_offset)
                    .expect("copied item end was already checked"),
            ));
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
                        .map_or((Frame::new(0), id.get()), |item| (item.start(), id.get()))
                });
            }
        }
        self.next_item_id = next_item_id;
        #[cfg(debug_assertions)]
        self.assert_consistent();
        Some(id_map)
    }

    pub(in crate::timeline) fn prepare_resize_items_from(
        &self,
        origins: &[TimelineItem],
        anchor_id: ItemId,
        edge: ResizeEdge,
        pointer: Frame,
        mode: ResizeMode,
    ) -> Option<Vec<crate::timeline::model::document::ItemEdit>> {
        if origins.is_empty() {
            return None;
        }

        let moving = origins.iter().map(|item| item.id).collect::<HashSet<_>>();
        if moving.len() != origins.len()
            || !moving
                .iter()
                .all(|id| self.items.contains_key(id) && self.item_layers.contains_key(id))
        {
            return None;
        }
        let anchor = origins.iter().find(|item| item.id == anchor_id)?;
        let anchor_edge = edge.item_frame(anchor, self.frame_rate);
        let requested_delta = i128::from(pointer.get()) - i128::from(anchor_edge);
        let mut minimum_delta = i128::MIN;
        let mut maximum_delta = i128::MAX;

        for origin in origins {
            let layer = self.item_layers[&origin.id];
            let start = origin.start().get();
            let end = origin.end_exclusive(self.frame_rate).get();
            let mut previous_end = 0;
            let mut next_start = u64::MAX;
            for candidate in self
                .layer_items
                .get(&layer)
                .into_iter()
                .flatten()
                .filter_map(|candidate| self.items.get(candidate))
            {
                if candidate.end_exclusive(self.frame_rate).get() <= start {
                    previous_end = previous_end.max(candidate.end_exclusive(self.frame_rate).get());
                }
                if candidate.start().get() >= end {
                    next_start = next_start.min(candidate.start().get());
                }
            }

            let (lower, upper) =
                edge.delta_bounds(origin, previous_end, next_start, self.frame_rate, mode);
            minimum_delta = minimum_delta.max(lower);
            maximum_delta = maximum_delta.min(upper);
        }

        if minimum_delta > maximum_delta {
            return None;
        }
        let delta = requested_delta.clamp(minimum_delta, maximum_delta);
        origins
            .iter()
            .map(|origin| {
                let mut edit = crate::timeline::model::document::ItemEdit::new(origin.clone());
                edge.resize_by(&mut edit.item, delta, self.frame_rate, mode)?;
                Some(edit)
            })
            .collect::<Option<Vec<_>>>()
    }

    pub(in crate::timeline) fn prepare_move_items_from(
        &self,
        origins: &[(ItemId, Frame, LayerId)],
        frame_delta: i64,
        layer_delta: i64,
    ) -> Option<Vec<crate::timeline::model::document::ItemEdit>> {
        if origins.is_empty() {
            return None;
        }
        let moving = origins.iter().map(|(id, _, _)| *id).collect::<HashSet<_>>();
        if moving.len() != origins.len() {
            return None;
        }
        origins
            .iter()
            .map(|(id, start, layer)| {
                let mut edit =
                    crate::timeline::model::document::ItemEdit::new(self.item(*id)?.clone());
                edit.item
                    .move_to(Frame::new(start.get().checked_add_signed(frame_delta)?));
                edit.layer = Some(LayerId::new(layer.get().checked_add_signed(layer_delta)?));
                Some(edit)
            })
            .collect()
    }
}
