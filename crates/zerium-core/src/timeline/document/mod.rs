//! Item storage and layer indexes. Placement and value-edit transactions stay inside this module.

mod editing;
mod placement;

pub use placement::{ResizeEdge, ResizeMode};

use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

use crate::animation::ScalarAnimations;
use crate::plugin::ItemSchema;
use crate::property::PropertyValues;

use super::{
    TimeMapping,
    aspect_ratio::AspectRatio,
    ids::{ItemId, LayerId},
    item::{TimelineItem, TimelineItemKind},
    settings::ProjectSettingsError,
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
                    .map_or((Frame::new(0), id.get()), |item| (item.start, id.get()))
            });
        }
        #[cfg(debug_assertions)]
        document.assert_consistent();
        document
    }

    pub fn frame_rate(&self) -> FrameRate {
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
                            "Item {} would be shorter than one frame at the new frame rate",
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
                "Items would overlap at the new frame rate",
            ));
        }
        Ok(document)
    }

    pub fn item(&self, id: ItemId) -> Option<&TimelineItem> {
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
        items.sort_unstable_by_key(|(layer, item)| (layer.get(), item.start, item.id.get()));
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

    pub(super) fn insert_item(
        &mut self,
        layer: LayerId,
        requested_start: Frame,
        duration: FrameDuration,
        create: impl FnOnce(ItemId, Frame, FrameDuration) -> TimelineItem,
    ) -> Option<ItemId> {
        let raw_id = self.next_item_id?;
        let id = ItemId(raw_id);
        let start = self.nearest_available_start(layer, requested_start, duration, None)?;
        let mut item = create(id, start, duration);
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

    pub fn add_item(
        &mut self,
        layer: LayerId,
        start: Frame,
        plugin_id: &str,
        item_id: &str,
        schema: Arc<ItemSchema>,
    ) -> Option<ItemId> {
        let properties = PropertyValues::from_properties(schema.properties());
        let aspect_ratio = schema
            .aspect_lock_default()
            .then(|| {
                schema.aspect_lock_property().and_then(|property| {
                    AspectRatio::from_value(properties.property(property.id())?)
                })
            })
            .flatten();
        let duration = schema
            .timeline()
            .and_then(|timeline| TimeMapping::from_properties(timeline, &properties).ok())
            .map_or_else(
                || self.frame_rate.seconds_to_duration(DEFAULT_ITEM_SECONDS),
                |mapping| mapping.timeline_duration(self.frame_rate),
            );
        let plugin_id = plugin_id.to_owned();
        let item_id = item_id.to_owned();
        self.insert_item(layer, start, duration, move |id, start, duration| {
            TimelineItem {
                id,
                start,
                duration,
                blend_mode: super::BlendMode::Normal,
                kind: TimelineItemKind::Plugin {
                    plugin_id,
                    item_id,
                    schema,
                },
                properties,
                animations: ScalarAnimations::default(),
                aspect_ratio,
                effects: Vec::new(),
            }
        })
    }

    pub(super) fn take_items(&mut self, ids: &HashSet<ItemId>) -> Vec<(LayerId, TimelineItem)> {
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

pub(super) fn retime_frame(
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
