//! Detached item edits. Resolve dependent timing and validate the whole batch before publication.

use std::{collections::HashSet, sync::Arc};

use crate::property::{PropertySchema, PropertyValue};
use crate::timeline::{
    EffectInstanceId, FrameRate, ItemId, TimeMapping, TimelineEditError, TimelineItem,
};

use super::TimelineDocument;

pub(in crate::timeline) struct ItemEdit {
    pub item: TimelineItem,
    previous_mapping: Option<TimeMapping>,
    synchronize_timeline: bool,
}

impl ItemEdit {
    pub(in crate::timeline) fn new(item: TimelineItem) -> Self {
        Self {
            previous_mapping: item.timeline_mapping(),
            item,
            synchronize_timeline: false,
        }
    }

    pub(in crate::timeline) fn set_property(
        &mut self,
        effect: Option<EffectInstanceId>,
        schema: &PropertySchema,
        value: Option<PropertyValue>,
    ) {
        if let Some(value) = value {
            let (properties, animations) = match effect {
                Some(id) => {
                    let effect = self
                        .item
                        .effect_mut(id)
                        .expect("prepared effect must exist");
                    (&mut effect.properties, &mut effect.animations)
                }
                None => (&mut self.item.properties, &mut self.item.animations),
            };
            if properties
                .set(schema, value)
                .expect("prepared value must satisfy its schema")
            {
                animations.retain_valid_for_property(schema.id(), properties.property(schema.id()));
            }
        } else {
            self.item.properties.remove(schema.id());
        }
        self.synchronize_timeline = true;
    }

    fn finish(mut self, frame_rate: FrameRate) -> Result<TimelineItem, TimelineEditError> {
        if self.synchronize_timeline {
            self.item
                .synchronize_timeline(self.previous_mapping, frame_rate)?;
        }
        self.item.validate_playback()?;
        self.item
            .start
            .get()
            .checked_add(self.item.duration.get())
            .ok_or(TimelineEditError::PlacementUnavailable)?;
        Ok(self.item)
    }
}

impl TimelineDocument {
    pub(in crate::timeline) fn prepare_item_edit(
        &self,
        id: ItemId,
    ) -> Result<ItemEdit, TimelineEditError> {
        self.item(id)
            .cloned()
            .map(ItemEdit::new)
            .ok_or(TimelineEditError::ItemNotFound(id))
    }

    pub(in crate::timeline) fn commit_item_edits(
        &mut self,
        edits: impl IntoIterator<Item = ItemEdit>,
    ) -> Result<bool, TimelineEditError> {
        let items = edits
            .into_iter()
            .map(|edit| edit.finish(self.frame_rate))
            .collect::<Result<Vec<_>, _>>()?;
        let ids = items.iter().map(|item| item.id).collect::<HashSet<_>>();
        if ids.len() != items.len() {
            return Err(TimelineEditError::PlacementUnavailable);
        }
        for (index, item) in items.iter().enumerate() {
            let layer = self
                .item_layers
                .get(&item.id)
                .copied()
                .ok_or(TimelineEditError::ItemNotFound(item.id))?;
            let original = self.item(item.id).expect("every indexed item exists");
            let placement_changed =
                original.start != item.start || original.duration != item.duration;
            if (placement_changed
                && self.overlaps_on_layer_excluding(layer, item.start, item.end_exclusive(), &ids))
                || items[index + 1..].iter().any(|other| {
                    self.item_layers.get(&other.id) == Some(&layer)
                        && item.start < other.end_exclusive()
                        && other.start < item.end_exclusive()
                })
            {
                return Err(TimelineEditError::PlacementUnavailable);
            }
        }
        let mut changed = false;
        for item in items {
            if self.item(item.id) != Some(&item) {
                self.items.insert(item.id, Arc::new(item));
                changed = true;
            }
        }
        #[cfg(debug_assertions)]
        self.assert_consistent();
        Ok(changed)
    }
}
