//! Detached item edits. Validate the whole batch before publication.

use std::{collections::HashSet, sync::Arc};

use crate::{
    property::{PropertySchema, PropertyValue},
    timeline::{EffectInstanceId, FrameRate, ItemId, LayerId, TimelineEditError, TimelineItem},
};

use super::TimelineDocument;

pub(in crate::timeline) struct ItemEdit {
    pub item: TimelineItem,
    pub layer: Option<LayerId>,
}

impl ItemEdit {
    pub(in crate::timeline) fn new(item: TimelineItem) -> Self {
        Self { layer: None, item }
    }

    pub(in crate::timeline) fn set_property(
        &mut self,
        effect: Option<EffectInstanceId>,
        schema: &PropertySchema,
        value: Option<PropertyValue>,
        frame_rate: FrameRate,
    ) -> bool {
        let previous_start = self.item.start();
        let previous_duration = self.item.duration(frame_rate);
        let stretch = effect.is_none()
            && self
                .item
                .schema()
                .and_then(|schema| schema.playback())
                .is_some_and(|playback| playback.playback_speed == schema.id());
        let owner = self
            .item
            .property_owner_mut(effect)
            .expect("prepared owner must exist");
        let changed = match value {
            Some(value) => owner
                .values
                .set(schema, value)
                .expect("prepared value must satisfy its schema"),
            None => owner.values.remove(schema.id()).is_some(),
        };
        if changed {
            let value = owner.values.resolve(schema);
            owner
                .animations
                .retain_valid_for_property(schema.id(), Some(&value));
        }
        if self.item.validate_playback(frame_rate).is_err() {
            return false;
        }
        let duration = self.item.duration(frame_rate);
        self.item.resize_animations(
            previous_start,
            previous_duration,
            duration,
            if stretch {
                crate::timeline::ResizeMode::Stretch
            } else {
                crate::timeline::ResizeMode::Trim
            },
        );
        true
    }

    fn finish(self, frame_rate: FrameRate) -> Result<TimelineItem, TimelineEditError> {
        self.item.validate_playback(frame_rate)?;
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
            let placement_changed = original.start() != item.start()
                || original.duration(self.frame_rate) != item.duration(self.frame_rate)
                || self.item_layers.get(&item.id) != Some(layer);
            if placement_changed
                && self.overlaps_on_layer_excluding(
                    *layer,
                    item.start(),
                    item.end_exclusive(self.frame_rate),
                    &ids,
                )
                || items[index + 1..].iter().any(|(other_layer, other)| {
                    other_layer == layer
                        && item.start() < other.end_exclusive(self.frame_rate)
                        && other.start() < item.end_exclusive(self.frame_rate)
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
            let moved = previous_layer != layer || self.items[&item.id].start() != item.start();
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
                ids.sort_unstable_by_key(|id| (self.items[id].start(), id.get()));
                if ids.is_empty() {
                    self.layer_items.remove(&layer);
                }
            }
        }
        #[cfg(debug_assertions)]
        self.assert_consistent();
    }
}
