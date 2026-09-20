use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

use crate::domain::animation::{ScalarAnimationAddress, ScalarAnimations};
use crate::domain::media::ImportedMedia;
use crate::domain::plugin::{EffectSchema, ItemSchema};
use crate::domain::property::PropertyValue;

use super::{
    ids::{EffectInstanceId, ItemId, LayerId},
    item::{EffectInstance, TimelineItem, TimelineItemKind, set_size_values, size_values},
    settings::ProjectSettingsError,
    time::{Frame, FrameDuration, FrameRate},
};

const DEFAULT_ITEM_SECONDS: f64 = 5.;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ResizeEdge {
    Left,
    Right,
}

impl ResizeEdge {
    pub(super) fn item_frame(self, item: &TimelineItem) -> u64 {
        match self {
            Self::Left => item.start.get(),
            Self::Right => item.end_exclusive().get(),
        }
    }

    fn delta_bounds(self, item: &TimelineItem, previous_end: u64, next_start: u64) -> (i128, i128) {
        let start = item.start.get();
        let end = item.end_exclusive().get();
        match self {
            Self::Left => (
                i128::from(previous_end) - i128::from(start),
                i128::from(end.saturating_sub(1)) - i128::from(start),
            ),
            Self::Right => (
                i128::from(start.saturating_add(1)) - i128::from(end),
                i128::from(next_start) - i128::from(end),
            ),
        }
    }

    fn resize_by(self, item: &mut TimelineItem, delta: i128) {
        let start = item.start.get();
        let end = item.end_exclusive().get();
        let (new_start, new_end) = match self {
            Self::Left => ((i128::from(start) + delta) as u64, end),
            Self::Right => (start, (i128::from(end) + delta) as u64),
        };
        let duration = FrameDuration::new_saturating(new_end - new_start);
        match self {
            Self::Left => item.trim_left_to(Frame(new_start), duration),
            Self::Right => item.trim_right_to(duration),
        }
    }
}

#[derive(Clone)]
pub(crate) struct TimelineDocument {
    frame_rate: FrameRate,
    items: HashMap<ItemId, Arc<TimelineItem>>,
    item_layers: HashMap<ItemId, LayerId>,
    layer_items: HashMap<LayerId, Vec<ItemId>>,
    next_item_id: Option<u64>,
}

impl Default for TimelineDocument {
    fn default() -> Self {
        Self::new(FrameRate::FPS_30)
    }
}

impl TimelineDocument {
    pub(crate) fn new(frame_rate: FrameRate) -> Self {
        Self {
            frame_rate,
            items: HashMap::new(),
            item_layers: HashMap::new(),
            layer_items: HashMap::new(),
            next_item_id: Some(1),
        }
    }

    pub(crate) fn from_items(frame_rate: FrameRate, items: Vec<(LayerId, TimelineItem)>) -> Self {
        let next_item_id = items
            .iter()
            .map(|(_, item)| item.id.get())
            .max()
            .unwrap_or(0)
            .checked_add(1)
            .filter(|id| *id != u64::MAX);
        let mut document = Self {
            frame_rate,
            items: HashMap::with_capacity(items.len()),
            item_layers: HashMap::with_capacity(items.len()),
            layer_items: HashMap::new(),
            next_item_id,
        };
        for (layer, item) in items {
            let id = item.id;
            document.items.insert(id, Arc::new(item));
            document.item_layers.insert(id, layer);
            document.layer_items.entry(layer).or_default().push(id);
        }
        for ids in document.layer_items.values_mut() {
            ids.sort_unstable_by_key(|id| {
                document
                    .items
                    .get(id)
                    .map_or((Frame::new(0), id.get()), |item| (item.start, id.get()))
            });
        }
        #[cfg(debug_assertions)]
        document.assert_consistent();
        document
    }

    pub(crate) fn frame_rate(&self) -> FrameRate {
        self.frame_rate
    }

    pub(super) fn retimed(&self, frame_rate: FrameRate) -> Result<Self, ProjectSettingsError> {
        if self.frame_rate == frame_rate {
            return Ok(self.clone());
        }
        let items = self
            .items
            .values()
            .map(|item| {
                let layer = self.item_layer(item.id).expect("every item has a layer");
                let start = retime_frame(item.start, self.frame_rate, frame_rate)?;
                let end = retime_frame(item.end_exclusive(), self.frame_rate, frame_rate)?;
                let duration = end
                    .get()
                    .checked_sub(start.get())
                    .and_then(FrameDuration::new)
                    .ok_or_else(|| {
                        ProjectSettingsError::out_of_range(format!(
                            "アイテム {} は変更後のフレームレートでは1フレーム未満になります",
                            item.id.get()
                        ))
                    })?;
                let mut item = item.as_ref().clone();
                item.start = start;
                item.duration = duration;
                Ok((layer, item))
            })
            .collect::<Result<Vec<_>, ProjectSettingsError>>()?;
        let document = Self::from_items(frame_rate, items);
        if document.layer_items.values().any(|ids| {
            ids.windows(2).any(|pair| {
                document.items[&pair[0]].end_exclusive() > document.items[&pair[1]].start
            })
        }) {
            return Err(ProjectSettingsError::out_of_range(
                "変更後のフレームレートではアイテム同士が重なります",
            ));
        }
        Ok(document)
    }

    pub(crate) fn item(&self, id: ItemId) -> Option<&TimelineItem> {
        self.items.get(&id).map(Arc::as_ref)
    }

    pub(super) fn item_mut(&mut self, id: ItemId) -> Option<&mut TimelineItem> {
        self.items.get_mut(&id).map(Arc::make_mut)
    }

    pub(super) fn item_layer(&self, id: ItemId) -> Option<LayerId> {
        self.item_layers.get(&id).copied()
    }

    pub(super) fn items(&self) -> impl Iterator<Item = &TimelineItem> {
        self.items.values().map(Arc::as_ref)
    }

    pub(super) fn items_mut(&mut self) -> impl Iterator<Item = &mut TimelineItem> {
        self.items.values_mut().map(Arc::make_mut)
    }

    pub(super) fn item_layouts(
        &self,
    ) -> impl Iterator<Item = (ItemId, LayerId, Frame, Frame)> + '_ {
        self.items.values().filter_map(|item| {
            let layer = self.item_layers.get(&item.id).copied()?;
            Some((item.id, layer, item.start, item.end_exclusive()))
        })
    }

    pub(super) fn overlaps_on_layer_excluding(
        &self,
        layer: LayerId,
        start: Frame,
        end: Frame,
        excluded: &HashSet<ItemId>,
    ) -> bool {
        self.layer_items
            .get(&layer)
            .into_iter()
            .flatten()
            .filter(|id| !excluded.contains(id))
            .filter_map(|id| self.items.get(id))
            .any(|item| start < item.end_exclusive() && item.start < end)
    }

    pub(crate) fn items_on_layer(&self, layer: LayerId) -> Vec<TimelineItem> {
        self.layer_items
            .get(&layer)
            .into_iter()
            .flatten()
            .filter_map(|id| self.items.get(id))
            .map(|item| item.as_ref().clone())
            .collect()
    }

    fn nearest_available_start(
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

    fn source_items_where(
        &self,
        mut include: impl FnMut(&TimelineItem) -> bool,
    ) -> Vec<(LayerId, TimelineItem)> {
        let mut items = self
            .items
            .values()
            .filter(|item| include(item))
            .filter_map(|item| {
                self.item_layers
                    .get(&item.id)
                    .copied()
                    .map(|layer| (layer, item.as_ref().clone()))
            })
            .collect::<Vec<_>>();
        items.sort_by(|(layer_a, item_a), (layer_b, item_b)| {
            layer_a
                .get()
                .cmp(&layer_b.get())
                .then_with(|| item_a.start.cmp(&item_b.start))
                .then_with(|| item_a.id.0.cmp(&item_b.id.0))
        });
        items
    }

    pub(super) fn source_items(&self) -> Vec<(LayerId, TimelineItem)> {
        self.source_items_where(|_| true)
    }

    pub(super) fn active_source_items_at_time(
        &self,
        time: super::time::TimelineTime,
    ) -> Vec<(LayerId, TimelineItem)> {
        self.source_items_where(|item| {
            let start = item.start.get() as f64;
            let end = item.end_exclusive().get() as f64;
            time.frames() >= start && time.frames() < end
        })
    }

    #[cfg(debug_assertions)]
    fn assert_consistent(&self) {
        debug_assert_eq!(self.items.len(), self.item_layers.len());
        debug_assert_eq!(
            self.items.len(),
            self.layer_items.values().map(Vec::len).sum::<usize>()
        );

        for (id, layer) in &self.item_layers {
            debug_assert!(self.items.contains_key(id));
            debug_assert!(
                self.layer_items
                    .get(layer)
                    .is_some_and(|ids| ids.contains(id))
            );
        }
        for (layer, ids) in &self.layer_items {
            for id in ids {
                debug_assert_eq!(self.item_layers.get(id), Some(layer));
            }

            let mut items = ids
                .iter()
                .filter_map(|id| self.items.get(id))
                .collect::<Vec<_>>();
            items.sort_unstable_by_key(|item| item.start);
            for pair in items.windows(2) {
                debug_assert!(pair[0].end_exclusive() <= pair[1].start);
            }
        }
    }

    fn add_item_from(
        &mut self,
        layer: LayerId,
        requested_start: Frame,
        duration: FrameDuration,
        create: impl FnOnce(ItemId, Frame, FrameDuration) -> Option<TimelineItem>,
    ) -> Option<ItemId> {
        let raw_id = self.next_item_id?;
        let id = ItemId(raw_id);
        let start = self.nearest_available_start(layer, requested_start, duration, None)?;
        let mut item = create(id, start, duration)?;
        item.id = id;
        item.start = start;
        item.duration = duration;

        self.items.insert(id, Arc::new(item));
        self.item_layers.insert(id, layer);
        self.layer_items.entry(layer).or_default().push(id);
        self.next_item_id = raw_id.checked_add(1).filter(|id| *id != u64::MAX);
        #[cfg(debug_assertions)]
        self.assert_consistent();
        Some(id)
    }

    pub(crate) fn add_item(
        &mut self,
        layer: LayerId,
        start: Frame,
        plugin_id: &str,
        item_id: &str,
        schema: Arc<ItemSchema>,
    ) -> Option<ItemId> {
        let properties = schema.default_property_values();
        let duration =
            FrameDuration::new_saturating(self.frame_rate.seconds_to_frame(DEFAULT_ITEM_SECONDS).0);
        let plugin_id = plugin_id.to_owned();
        let item_id = item_id.to_owned();
        self.add_item_from(layer, start, duration, move |id, start, duration| {
            Some(TimelineItem {
                id,
                start,
                duration,
                kind: TimelineItemKind::Plugin {
                    plugin_id,
                    item_id,
                    schema,
                },
                assets: HashMap::new(),
                properties,
                animations: ScalarAnimations::default(),
                aspect_ratio_locked: false,
                effects: Vec::new(),
            })
        })
    }

    pub(super) fn add_generated_item(
        &mut self,
        layer: LayerId,
        start: Frame,
        duration: FrameDuration,
        create: impl FnOnce(ItemId, Frame, FrameDuration) -> Option<TimelineItem>,
    ) -> Option<ItemId> {
        self.add_item_from(layer, start, duration, create)
    }

    pub(super) fn insert_item_copies(
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

    pub(super) fn take_items(&mut self, ids: &HashSet<ItemId>) -> Vec<(LayerId, TimelineItem)> {
        let mut taken = Vec::new();
        for id in ids {
            let Some(layer) = self.item_layers.remove(id) else {
                continue;
            };
            if let Some(item) = self.items.remove(id) {
                taken.push((layer, Arc::unwrap_or_clone(item)));
            }
            if let Some(layer_items) = self.layer_items.get_mut(&layer) {
                layer_items.retain(|candidate| candidate != id);
            }
        }
        self.layer_items.retain(|_, ids| !ids.is_empty());
        taken
    }

    pub(crate) fn set_item_asset(&mut self, id: ItemId, imported: ImportedMedia) -> bool {
        if imported.asset.validate().is_err() {
            return false;
        }
        let Some(item) = self.items.get(&id) else {
            return false;
        };
        if item.plugin_id() != Some(imported.plugin_id.as_str())
            || item.item_id() != Some(imported.item_id.as_str())
        {
            return false;
        }
        let Some(file) = item
            .schema()
            .and_then(|schema| schema.file(&imported.input_id))
        else {
            return false;
        };
        if file.reader() != imported.asset.reader_id
            || file.media_type() != imported.asset.kind.media_type()
        {
            return false;
        }
        let duration = FrameDuration::new_saturating(
            self.frame_rate
                .seconds_to_frame(imported.asset.duration.as_secs_f64())
                .get(),
        );
        let first_asset = item.assets.is_empty();
        let duration = if first_asset {
            duration
        } else {
            FrameDuration::new_saturating(item.duration.get().max(duration.get()))
        };
        let Some(layer) = self.item_layers.get(&id).copied() else {
            return false;
        };
        let Some(start) = self.nearest_available_start(layer, item.start, duration, Some(id))
        else {
            return false;
        };
        let Some(item) = self.items.get_mut(&id).map(Arc::make_mut) else {
            return false;
        };
        item.trim_right_to(duration);
        item.start = start;
        if first_asset {
            let dimensions = imported
                .asset
                .kind
                .dimensions()
                .map(|[width, height]| [width as f32, height as f32]);
            if let Some(source_size) = dimensions
                && let Some(schema) = item.schema_arc().cloned()
                && let Some(bounds) = size_values(&item.properties, &schema)
            {
                let scale = (bounds[0] / source_size[0]).min(bounds[1] / source_size[1]);
                set_size_values(
                    &mut item.properties,
                    &schema,
                    [source_size[0] * scale, source_size[1] * scale],
                );
            }
        }
        item.assets.insert(imported.input_id, imported.asset);
        #[cfg(debug_assertions)]
        self.assert_consistent();
        true
    }

    pub(crate) fn remove_item(&mut self, id: ItemId) -> bool {
        let Some(layer) = self.item_layers.remove(&id) else {
            return false;
        };
        let removed = self.items.remove(&id).is_some();
        let remove_layer = if let Some(ids) = self.layer_items.get_mut(&layer) {
            ids.retain(|candidate| *candidate != id);
            ids.is_empty()
        } else {
            false
        };
        if remove_layer {
            self.layer_items.remove(&layer);
        }
        #[cfg(debug_assertions)]
        self.assert_consistent();
        removed
    }

    pub(crate) fn update_item_property(
        &mut self,
        id: ItemId,
        property_id: &str,
        value: PropertyValue,
    ) -> bool {
        let Some(item) = self.items.get_mut(&id).map(Arc::make_mut) else {
            return false;
        };
        let Some(schema) = item.schema_arc().cloned() else {
            return false;
        };
        let Some(property) = schema.property(property_id) else {
            return false;
        };
        if !property.is_editable(None) {
            return false;
        }
        // Resolve a locked size as one value. Writing width first can leave the old
        // height behind if the derived height is outside its contract.
        let value = if item.preserves_aspect_ratio()
            && schema.is_size_property(property_id)
            && property.ty().allows(&value)
            && let Some(ratio) = item.current_aspect_ratio(&schema)
        {
            let requested = [
                value
                    .scalar_at(Some(0))
                    .and_then(PropertyValue::numeric_scalar)
                    .unwrap() as f32,
                value
                    .scalar_at(Some(1))
                    .and_then(PropertyValue::numeric_scalar)
                    .unwrap() as f32,
            ];
            PropertyValue::f32_tuple(super::item::size_with_derived_height(
                requested, ratio, &schema,
            ))
        } else {
            value
        };
        let mut changed = item.properties.set(property, value).unwrap_or(false);
        if changed {
            changed |= item.animations.retain_valid(&item.properties);
        }

        changed
    }

    pub(crate) fn update_item_aspect_ratio_locked(&mut self, id: ItemId, locked: bool) -> bool {
        let Some(item) = self.items.get_mut(&id).map(Arc::make_mut) else {
            return false;
        };
        let Some(schema) = item.schema_arc().cloned() else {
            return false;
        };
        if !schema.supports_aspect_ratio_lock()
            || schema
                .size_property()
                .is_none_or(|property| !property.is_editable(None))
            || item.aspect_ratio_locked == locked
        {
            return false;
        }
        let aspect_ratio = item.current_aspect_ratio(&schema);
        item.aspect_ratio_locked = locked;
        if locked && let Some(size) = schema.size_property() {
            item.animations
                .remove(&ScalarAnimationAddress::new(size.id.clone(), None, Some(1)));
        }
        if locked && let Some(aspect_ratio) = aspect_ratio {
            item.constrain_size_to_aspect_ratio(aspect_ratio);
        }
        true
    }

    pub(super) fn add_item_effect(
        &mut self,
        id: ItemId,
        instance_id: EffectInstanceId,
        plugin_id: &str,
        effect_id: &str,
        schema: Arc<EffectSchema>,
    ) -> bool {
        let properties = schema.default_property_values();
        let Some(item) = self.items.get_mut(&id).map(Arc::make_mut) else {
            return false;
        };
        if item.scene_id().is_none() {
            let Some(item_schema) = item.schema() else {
                return false;
            };
            if item_schema.visual().is_none() {
                return false;
            }
        }
        item.effects.push(EffectInstance {
            id: instance_id,
            plugin_id: plugin_id.to_owned(),
            effect_id: effect_id.to_owned(),
            properties,
            animations: ScalarAnimations::default(),
            schema,
        });
        true
    }

    pub(crate) fn update_item_effect_property(
        &mut self,
        item_id: ItemId,
        effect_id: EffectInstanceId,
        property_id: &str,
        value: PropertyValue,
    ) -> bool {
        let Some(effect) = self
            .items
            .get_mut(&item_id)
            .map(Arc::make_mut)
            .and_then(|item| {
                item.effects
                    .iter_mut()
                    .find(|effect| effect.id == effect_id)
            })
        else {
            return false;
        };
        let schema = effect.schema.clone();
        let Some(property) = schema.property(property_id) else {
            return false;
        };
        if !property.is_editable(None) {
            return false;
        }
        let mut changed = effect.properties.set(property, value).unwrap_or(false);
        if changed {
            changed |= effect.animations.retain_valid(&effect.properties);
        }
        changed
    }

    pub(crate) fn remove_item_effect(
        &mut self,
        item_id: ItemId,
        effect_id: EffectInstanceId,
    ) -> bool {
        let Some(item) = self.items.get_mut(&item_id).map(Arc::make_mut) else {
            return false;
        };
        let previous_len = item.effects.len();
        item.effects.retain(|effect| effect.id != effect_id);
        item.effects.len() != previous_len
    }

    pub(crate) fn move_item_effect(
        &mut self,
        item_id: ItemId,
        effect_id: EffectInstanceId,
        target_index: usize,
    ) -> bool {
        let Some(item) = self.items.get_mut(&item_id).map(Arc::make_mut) else {
            return false;
        };
        let Some(source_index) = item
            .effects
            .iter()
            .position(|effect| effect.id == effect_id)
        else {
            return false;
        };
        if target_index >= item.effects.len() || source_index == target_index {
            return false;
        }
        let effect = item.effects.remove(source_index);
        item.effects.insert(target_index, effect);
        true
    }

    pub(crate) fn resize_items_from(
        &mut self,
        origins: &[TimelineItem],
        anchor_id: ItemId,
        edge: ResizeEdge,
        pointer: Frame,
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
                .filter(|candidate| !moving.contains(candidate))
                .filter_map(|candidate| self.items.get(candidate))
            {
                if candidate.end_exclusive().get() <= start {
                    previous_end = previous_end.max(candidate.end_exclusive().get());
                }
                if candidate.start.get() >= end {
                    next_start = next_start.min(candidate.start.get());
                }
            }

            let (lower, upper) = edge.delta_bounds(origin, previous_end, next_start);
            minimum_delta = minimum_delta.max(lower);
            maximum_delta = maximum_delta.min(upper);
        }

        let delta = requested_delta.clamp(minimum_delta, maximum_delta);
        let mut changed = false;

        for origin in origins {
            let mut item = origin.clone();
            edge.resize_by(&mut item, delta);
            if self.items[&origin.id].as_ref() != &item {
                self.items.insert(origin.id, Arc::new(item));
                changed = true;
            }
        }
        #[cfg(debug_assertions)]
        self.assert_consistent();
        changed
    }

    pub(crate) fn move_items_from(
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

        for (index, (id, start, end, layer)) in targets.iter().enumerate() {
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
            if collides_with_stationary_item || !moving.contains(id) {
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

        let current_layers = moving
            .iter()
            .filter_map(|id| self.item_layers.get(id).copied())
            .collect::<HashSet<_>>();
        for layer in current_layers {
            let remove_layer = if let Some(ids) = self.layer_items.get_mut(&layer) {
                ids.retain(|id| !moving.contains(id));
                ids.is_empty()
            } else {
                false
            };
            if remove_layer {
                self.layer_items.remove(&layer);
            }
        }
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

pub(super) fn retime_frame(
    frame: Frame,
    source: FrameRate,
    target: FrameRate,
) -> Result<Frame, ProjectSettingsError> {
    let numerator = u128::from(frame.get())
        .checked_mul(u128::from(target.numerator()))
        .and_then(|value| value.checked_mul(u128::from(source.denominator())))
        .ok_or_else(|| ProjectSettingsError::out_of_range("フレーム位置が大きすぎます"))?;
    let denominator = u128::from(source.numerator()) * u128::from(target.denominator());
    let rounded = numerator
        .checked_add(denominator / 2)
        .ok_or_else(|| ProjectSettingsError::out_of_range("フレーム位置が大きすぎます"))?
        / denominator;
    let frame = u64::try_from(rounded)
        .map_err(|_| ProjectSettingsError::out_of_range("フレーム位置が大きすぎます"))?;
    Ok(Frame::new(frame))
}
