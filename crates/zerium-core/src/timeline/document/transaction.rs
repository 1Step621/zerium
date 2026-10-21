//! Detached item edits. Resolve dependent timing and validate the whole batch before publication.

use std::{collections::HashSet, sync::Arc};

use crate::property::{PropertySchema, PropertyValue};
use crate::timeline::{
    EffectInstanceId, FrameRate, ItemId, LayerId, TimeMapping, TimelineEditError, TimelineItem,
};

use super::TimelineDocument;

pub(in crate::timeline) struct ItemEdit {
    pub item: TimelineItem,
    pub layer: Option<LayerId>,
    previous_mapping: Option<TimeMapping>,
    synchronize_timeline: bool,
}

impl ItemEdit {
    pub(in crate::timeline) fn new(item: TimelineItem) -> Self {
        Self {
            previous_mapping: item.timeline_mapping(),
            layer: None,
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
            let owner = self
                .item
                .property_owner_mut(effect)
                .expect("prepared owner must exist");
            let (properties, animations) = (owner.values, owner.animations);
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

/// Fully validated replacements. Publication has no remaining failure path.
pub(in crate::timeline) struct PreparedItemEdits {
    items: Vec<(LayerId, TimelineItem)>,
    pub media_dependencies_changed: bool,
}

impl PreparedItemEdits {
    pub(in crate::timeline) fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
}

fn file_inputs_changed(before: &TimelineItem, after: &TimelineItem) -> bool {
    let files = |item: &TimelineItem| {
        let mut files = item
            .properties
            .files()
            .map(|(path, file)| (None, path, file.to_owned()))
            .collect::<Vec<_>>();
        files.extend(item.effects.iter().flat_map(|effect| {
            effect
                .properties
                .files()
                .map(move |(path, file)| (Some(effect.id), path, file.to_owned()))
        }));
        files.into_iter().collect::<HashSet<_>>()
    };
    before.kind != after.kind
        || before.effects.len() != after.effects.len()
        || files(before) != files(after)
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

    pub(in crate::timeline) fn prepare_item_edits(
        &self,
        edits: impl IntoIterator<Item = ItemEdit>,
    ) -> Result<PreparedItemEdits, TimelineEditError> {
        let items = edits
            .into_iter()
            .map(|edit| {
                let layer = edit
                    .layer
                    .or_else(|| self.item_layers.get(&edit.item.id).copied())
                    .ok_or(TimelineEditError::ItemNotFound(edit.item.id))?;
                Ok((layer, edit.finish(self.frame_rate)?))
            })
            .collect::<Result<Vec<_>, _>>()?;
        let ids = items
            .iter()
            .map(|(_, item)| item.id)
            .collect::<HashSet<_>>();
        if ids.len() != items.len() {
            return Err(TimelineEditError::PlacementUnavailable);
        }
        for (index, (layer, item)) in items.iter().enumerate() {
            let original = self
                .item(item.id)
                .ok_or(TimelineEditError::ItemNotFound(item.id))?;
            let placement_changed = original.start != item.start
                || original.duration != item.duration
                || self.item_layers.get(&item.id) != Some(layer);
            if placement_changed
                && self.overlaps_on_layer_excluding(*layer, item.start, item.end_exclusive(), &ids)
                || items[index + 1..].iter().any(|(other_layer, other)| {
                    other_layer == layer
                        && item.start < other.end_exclusive()
                        && other.start < item.end_exclusive()
                })
            {
                return Err(TimelineEditError::PlacementUnavailable);
            }
        }
        let mut media_dependencies_changed = false;
        let items = items
            .into_iter()
            .filter(|(layer, item)| {
                let original = self.item(item.id).expect("validated item");
                if original == item && self.item_layers.get(&item.id) == Some(layer) {
                    return false;
                }
                media_dependencies_changed |= file_inputs_changed(original, item);
                true
            })
            .collect();
        Ok(PreparedItemEdits {
            items,
            media_dependencies_changed,
        })
    }

    pub(in crate::timeline) fn publish_item_edits(&mut self, edits: PreparedItemEdits) {
        let mut affected = HashSet::new();
        for (layer, item) in edits.items {
            let previous_layer = self.item_layers[&item.id];
            let moved = previous_layer != layer || self.items[&item.id].start != item.start;
            if moved {
                affected.insert(previous_layer);
                affected.insert(layer);
            }
            if previous_layer != layer {
                self.layer_items
                    .get_mut(&previous_layer)
                    .expect("indexed layer")
                    .retain(|id| *id != item.id);
                self.layer_items.entry(layer).or_default().push(item.id);
                self.item_layers.insert(item.id, layer);
            }
            self.items.insert(item.id, Arc::new(item));
        }
        for layer in affected {
            if let Some(ids) = self.layer_items.get_mut(&layer) {
                ids.sort_unstable_by_key(|id| (self.items[id].start, id.get()));
                if ids.is_empty() {
                    self.layer_items.remove(&layer);
                }
            }
        }
        #[cfg(debug_assertions)]
        self.assert_consistent();
    }
}
