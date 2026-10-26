//! Item storage and layer indexes. Placement and value-edit transactions stay inside this module.

mod editing;
mod placement;
mod transaction;

pub use self::placement::{ResizeEdge, ResizeMode};
pub(in crate::timeline) use self::{placement::CopyPlacement, transaction::ItemEdit};

use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

use crate::{animation::ScalarAnimations, plugin::ItemSchema, property::PropertyOverrides};

use super::{
    ItemId, LayerId,
    item::{AspectRatio, TimelineItem, TimelineItemKind},
    project::ProjectSettingsError,
    time::{Frame, FrameDuration, FrameRate},
};

const DEFAULT_ITEM_SECONDS: f64 = 5.;

#[derive(Clone)]
pub struct TimelineDocument {
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
    pub fn new(frame_rate: FrameRate) -> Self {
        Self {
            frame_rate,
            items: HashMap::new(),
            item_layers: HashMap::new(),
            layer_items: HashMap::new(),
            next_item_id: Some(1),
        }
    }

    pub fn from_items(frame_rate: FrameRate, items: Vec<(LayerId, TimelineItem)>) -> Self {
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
                    .map_or((Frame::new(0), id.get()), |item| (item.start(), id.get()))
            });
        }
        #[cfg(debug_assertions)]
        document.assert_consistent();
        document
    }

    pub fn frame_rate(&self) -> FrameRate {
        self.frame_rate
    }

    pub(crate) fn retimed(&self, frame_rate: FrameRate) -> Result<Self, ProjectSettingsError> {
        if self.frame_rate == frame_rate {
            return Ok(self.clone());
        }
        let items = self
            .items
            .values()
            .map(|item| {
                let layer = self.item_layer(item.id).expect("every item has a layer");
                let start = retime_frame(item.start(), self.frame_rate, frame_rate)?;
                let previous_duration = item.duration(self.frame_rate);
                let duration = if item.fixed_duration.is_none() {
                    item.duration(frame_rate)
                } else {
                    let end = retime_frame(
                        item.end_exclusive(self.frame_rate),
                        self.frame_rate,
                        frame_rate,
                    )?;
                    end.get()
                        .checked_sub(start.get())
                        .and_then(FrameDuration::new)
                        .ok_or_else(|| {
                            ProjectSettingsError::out_of_range(format!(
                                "Item {} would be shorter than one frame at the new frame rate",
                                item.id.get()
                            ))
                        })?
                };
                let mut item = item.as_ref().clone();
                item.retime_to(start, duration, previous_duration);
                item.validate_playback(frame_rate)
                    .map_err(|error| ProjectSettingsError::out_of_range(error.to_string()))?;
                Ok((layer, item))
            })
            .collect::<Result<Vec<_>, ProjectSettingsError>>()?;
        let mut placements = items
            .iter()
            .map(|(layer, item)| (layer.get(), item.start(), item.end_exclusive(frame_rate)))
            .collect::<Vec<_>>();
        placements.sort_unstable();
        if placements
            .windows(2)
            .any(|pair| pair[0].0 == pair[1].0 && pair[0].2 > pair[1].1)
        {
            return Err(ProjectSettingsError::out_of_range(
                "Items would overlap at the new frame rate",
            ));
        }
        Ok(Self::from_items(frame_rate, items))
    }

    pub fn item(&self, id: ItemId) -> Option<&TimelineItem> {
        self.items.get(&id).map(Arc::as_ref)
    }

    pub(in crate::timeline) fn item_mut(&mut self, id: ItemId) -> Option<&mut TimelineItem> {
        self.items.get_mut(&id).map(Arc::make_mut)
    }

    pub(in crate::timeline) fn item_layer(&self, id: ItemId) -> Option<LayerId> {
        self.item_layers.get(&id).copied()
    }

    pub(in crate::timeline) fn items(&self) -> impl Iterator<Item = &TimelineItem> {
        self.items.values().map(Arc::as_ref)
    }

    pub(in crate::timeline) fn end_frame_exclusive(&self) -> Frame {
        self.items()
            .map(|item| item.end_exclusive(self.frame_rate))
            .max()
            .unwrap_or(Frame::new(0))
    }

    pub(in crate::timeline) fn item_layouts(
        &self,
    ) -> impl Iterator<Item = (ItemId, LayerId, Frame, Frame)> + '_ {
        self.items.values().filter_map(|item| {
            let layer = self.item_layers.get(&item.id).copied()?;
            Some((
                item.id,
                layer,
                item.start(),
                item.end_exclusive(self.frame_rate),
            ))
        })
    }

    pub(in crate::timeline) fn overlaps_on_layer_excluding(
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
            .any(|item| start < item.end_exclusive(self.frame_rate) && item.start() < end)
    }

    pub fn items_on_layer(&self, layer: LayerId) -> Vec<TimelineItem> {
        self.layer_items
            .get(&layer)
            .into_iter()
            .flatten()
            .filter_map(|id| self.items.get(id))
            .map(|item| item.as_ref().clone())
            .collect()
    }

    fn source_items_where(
        &self,
        mut include: impl FnMut(&TimelineItem) -> bool,
    ) -> Vec<(LayerId, &TimelineItem)> {
        let mut items = self
            .items
            .values()
            .filter(|item| include(item))
            .filter_map(|item| {
                self.item_layers
                    .get(&item.id)
                    .copied()
                    .map(|layer| (layer, item.as_ref()))
            })
            .collect::<Vec<_>>();
        items.sort_unstable_by_key(|(layer, item)| (layer.get(), item.start(), item.id.get()));
        items
    }

    pub(crate) fn source_items(&self) -> Vec<(LayerId, TimelineItem)> {
        self.source_items_where(|_| true)
            .into_iter()
            .map(|(layer, item)| (layer, item.clone()))
            .collect()
    }

    pub(in crate::timeline) fn active_source_items_at_time(
        &self,
        time: crate::timeline::model::time::TimelineTime,
    ) -> Vec<(LayerId, TimelineItem)> {
        self.active_source_item_refs_at_time(time)
            .into_iter()
            .map(|(layer, item)| (layer, item.clone()))
            .collect()
    }

    pub(in crate::timeline) fn active_source_item_ids_at_time(
        &self,
        time: crate::timeline::model::time::TimelineTime,
    ) -> Vec<ItemId> {
        self.active_source_item_refs_at_time(time)
            .into_iter()
            .map(|(_, item)| item.id)
            .collect()
    }

    fn active_source_item_refs_at_time(
        &self,
        time: crate::timeline::model::time::TimelineTime,
    ) -> Vec<(LayerId, &TimelineItem)> {
        self.source_items_where(|item| {
            let start = item.start().get() as f64;
            let end = item.end_exclusive(self.frame_rate).get() as f64;
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
            items.sort_unstable_by_key(|item| item.start());
            for pair in items.windows(2) {
                debug_assert!(pair[0].end_exclusive(self.frame_rate) <= pair[1].start());
            }
        }
    }

    pub(in crate::timeline) fn insert_item(
        &mut self,
        layer: LayerId,
        requested_start: Frame,
        duration: FrameDuration,
        create: impl FnOnce(ItemId, Frame, FrameDuration) -> TimelineItem,
    ) -> Option<ItemId> {
        let raw_id = self.next_item_id?;
        let id = ItemId(raw_id);
        let start = self.nearest_available_start(layer, requested_start, duration, None)?;
        let item = create(id, start, duration);
        item.validate_playback(self.frame_rate).ok()?;

        self.items.insert(id, Arc::new(item));
        self.item_layers.insert(id, layer);
        self.layer_items.entry(layer).or_default().push(id);
        self.next_item_id = raw_id.checked_add(1).filter(|id| *id != u64::MAX);
        #[cfg(debug_assertions)]
        self.assert_consistent();
        Some(id)
    }

    pub fn add_item(
        &mut self,
        layer: LayerId,
        start: Frame,
        plugin_id: &str,
        item_id: &str,
        schema: Arc<ItemSchema>,
        mut properties: PropertyOverrides,
    ) -> Option<ItemId> {
        crate::plugin::initialize_frame_rates(
            schema.inputs(),
            schema.properties(),
            &mut properties,
            self.frame_rate,
        );
        let aspect_ratio = schema
            .aspect_lock_default()
            .then(|| {
                schema
                    .aspect_lock_property()
                    .and_then(|property| AspectRatio::from_value(&properties.resolve(property)))
            })
            .flatten();
        let fixed_duration = schema
            .playback()
            .is_none()
            .then(|| self.frame_rate.seconds_to_duration(DEFAULT_ITEM_SECONDS));
        let duration = if let Some(playback) = schema.playback() {
            playback
                .mapping(&properties.materialize(schema.properties()))
                .ok()?
                .timeline_duration(self.frame_rate)
        } else {
            fixed_duration.expect("fixed clip")
        };
        let plugin_id = plugin_id.to_owned();
        let item_id = item_id.to_owned();
        self.insert_item(layer, start, duration, move |id, start, _| TimelineItem {
            id,
            start,
            fixed_duration,
            blend_mode: crate::timeline::BlendMode::Normal,
            kind: TimelineItemKind::Plugin {
                plugin_id,
                item_id,
                schema,
            },
            properties,
            animations: ScalarAnimations::default(),
            aspect_ratio,
            effects: Vec::new(),
        })
    }

    pub(in crate::timeline) fn take_items(
        &mut self,
        ids: &HashSet<ItemId>,
    ) -> Vec<(LayerId, TimelineItem)> {
        ids.iter()
            .filter_map(|id| self.remove_item_entry(*id))
            .map(|(layer, item)| (layer, Arc::unwrap_or_clone(item)))
            .collect()
    }

    fn remove_item_entry(&mut self, id: ItemId) -> Option<(LayerId, Arc<TimelineItem>)> {
        let layer = self.item_layers.remove(&id)?;
        let item = self.items.remove(&id);
        if let Some(ids) = self.layer_items.get_mut(&layer) {
            ids.retain(|candidate| *candidate != id);
            if ids.is_empty() {
                self.layer_items.remove(&layer);
            }
        }
        item.map(|item| (layer, item))
    }

    pub fn remove_item(&mut self, id: ItemId) -> bool {
        let removed = self.remove_item_entry(id).is_some();
        #[cfg(debug_assertions)]
        self.assert_consistent();
        removed
    }
}

pub(in crate::timeline) fn retime_frame(
    frame: Frame,
    source: FrameRate,
    target: FrameRate,
) -> Result<Frame, ProjectSettingsError> {
    let numerator = u128::from(frame.get())
        .checked_mul(u128::from(target.numerator()))
        .and_then(|value| value.checked_mul(u128::from(source.denominator())))
        .ok_or_else(|| ProjectSettingsError::out_of_range("Frame position is too large"))?;
    let denominator = u128::from(source.numerator()) * u128::from(target.denominator());
    let rounded = numerator
        .checked_add(denominator / 2)
        .ok_or_else(|| ProjectSettingsError::out_of_range("Frame position is too large"))?
        / denominator;
    let frame = u64::try_from(rounded)
        .map_err(|_| ProjectSettingsError::out_of_range("Frame position is too large"))?;
    Ok(Frame::new(frame))
}
