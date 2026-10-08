//! Property transactions, media initialization, and effect edits.

use std::{collections::HashMap, sync::Arc};

use crate::animation::ScalarAnimations;
use crate::media::ImportedFile;
use crate::plugin::EffectSchema;
use crate::property::{PropertySchema, PropertyValue, PropertyValues};
use crate::timeline::item::{EffectInstance, set_size_values, size_values};
use crate::timeline::{
    AspectRatio, EffectInstanceId, ItemId, TimeMapping, TimelineEditError, TimelineItem,
};

use super::{ResizeMode, TimelineDocument};

impl TimelineDocument {
    pub fn import_item_file(&mut self, id: ItemId, imported: ImportedFile) -> bool {
        let Some(item) = self.items.get(&id) else {
            return false;
        };
        if item.plugin_id() != Some(imported.plugin_id.as_str())
            || item.item_id() != Some(imported.source_id.as_str())
        {
            return false;
        }
        let Some(property) = item
            .schema()
            .and_then(|schema| schema.file_property(&imported.property_id))
        else {
            return false;
        };
        let schema = item.schema_arc().expect("plugin item").clone();
        let initial_duration = imported.file.initial_duration();
        let mut item = item.as_ref().clone();
        if item
            .properties
            .set(
                property,
                PropertyValue::File(Some(imported.file.path.clone())),
            )
            .is_err()
        {
            return false;
        }
        let dimensions = schema
            .media_sources()
            .filter(|source| source.file == imported.property_id)
            .filter_map(|source| imported.file.asset(source.reader, source.input.target()))
            .find_map(|asset| asset.kind.dimensions());
        if let Some(source_duration) = initial_duration
            && let Some(mapping) = item.timeline_mapping()
        {
            let Ok(mapping) =
                TimeMapping::new(0., source_duration.as_secs_f32(), mapping.speed() as f32)
            else {
                return false;
            };
            let Some(mapping) = item.store_timeline_mapping(mapping) else {
                return false;
            };
            item.set_interval(
                item.start,
                mapping.timeline_duration(self.frame_rate),
                ResizeMode::Trim,
            );
        }
        let duration = item.duration;
        let Some(layer) = self.item_layers.get(&id).copied() else {
            return false;
        };
        let Some(start) = self.nearest_available_start(layer, item.start, duration, Some(id))
        else {
            return false;
        };
        item.start = start;
        let dimensions = dimensions.map(|[width, height]| [width as f32, height as f32]);
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
        if item.validate_playback().is_err() {
            return false;
        }
        self.items.insert(id, Arc::new(item));
        #[cfg(debug_assertions)]
        self.assert_consistent();
        true
    }

    /// Prepare ordinary property edits, including their effects on timeline placement,
    /// before committing the complete selection.
    pub(in crate::timeline) fn set_properties(
        &mut self,
        edits: &[(
            ItemId,
            Option<EffectInstanceId>,
            PropertySchema,
            Option<PropertyValue>,
        )],
    ) -> Result<bool, TimelineEditError> {
        let mut updates = HashMap::new();
        for (id, effect, property, value) in edits {
            let original = self
                .items
                .get(id)
                .ok_or(TimelineEditError::ItemNotFound(*id))?;
            let item = updates
                .entry(*id)
                .or_insert_with(|| original.as_ref().clone());
            Self::set_item_property(item, *effect, property, value.clone());
        }
        for (id, item) in &mut updates {
            item.synchronize_timeline(self.items[id].timeline_mapping(), self.frame_rate)?;
            item.validate_playback()?;
            item.start
                .get()
                .checked_add(item.duration.get())
                .ok_or(TimelineEditError::PlacementUnavailable)?;
        }
        for ids in self.layer_items.values() {
            let mut items = ids
                .iter()
                .filter_map(|id| {
                    updates
                        .get(id)
                        .or_else(|| self.items.get(id).map(AsRef::as_ref))
                })
                .collect::<Vec<_>>();
            items.sort_unstable_by_key(|item| item.start);
            if items
                .windows(2)
                .any(|pair| pair[0].end_exclusive() > pair[1].start)
            {
                return Err(TimelineEditError::PlacementUnavailable);
            }
        }
        let mut changed = false;
        for (id, item) in updates {
            if self.items[&id].as_ref() != &item {
                self.items.insert(id, Arc::new(item));
                changed = true;
            }
        }
        Ok(changed)
    }

    /// Commit a value whose edit permissions and constraints were checked for
    /// the entire selection by the command layer.
    fn set_item_property(
        item: &mut TimelineItem,
        effect_id: Option<EffectInstanceId>,
        property: &PropertySchema,
        value: Option<PropertyValue>,
    ) {
        let Some(value) = value else {
            item.properties.remove(property.id());
            return;
        };
        let (properties, animations) = match effect_id {
            Some(id) => {
                let effect = item.effect_mut(id).expect("prepared effect must exist");
                (&mut effect.properties, &mut effect.animations)
            }
            None => (&mut item.properties, &mut item.animations),
        };
        let changed = properties
            .set(property, value)
            .expect("prepared value must satisfy its schema");
        if changed {
            animations.retain_valid_for_property(property.id(), properties.property(property.id()));
        }
    }

    pub(in crate::timeline) fn set_item_aspect_ratio(
        &mut self,
        id: ItemId,
        effect_id: Option<EffectInstanceId>,
        ratio: Option<AspectRatio>,
    ) -> bool {
        let item = self.item_mut(id).expect("prepared item must exist");
        let current = match effect_id {
            Some(id) => {
                &mut item
                    .effect_mut(id)
                    .expect("prepared effect must exist")
                    .aspect_ratio
            }
            None => &mut item.aspect_ratio,
        };
        if *current == ratio {
            return false;
        }
        *current = ratio;
        true
    }

    pub(in crate::timeline) fn add_item_effect(
        &mut self,
        id: ItemId,
        instance_id: EffectInstanceId,
        plugin_id: &str,
        effect_id: &str,
        schema: Arc<EffectSchema>,
    ) -> bool {
        let properties = PropertyValues::from_properties(schema.properties());
        let aspect_ratio = schema
            .aspect_lock_default()
            .then(|| {
                schema.aspect_lock_property().and_then(|property| {
                    AspectRatio::from_value(properties.property(property.id())?)
                })
            })
            .flatten();
        let Some(item) = self.items.get_mut(&id).map(Arc::make_mut) else {
            return false;
        };
        if item.scene_id().is_none() {
            let Some(item_schema) = item.schema() else {
                return false;
            };
            if item_schema.shader().is_none() {
                return false;
            }
        }
        item.effects.push(EffectInstance {
            id: instance_id,
            plugin_id: plugin_id.to_owned(),
            effect_id: effect_id.to_owned(),
            properties,
            animations: ScalarAnimations::default(),
            aspect_ratio,
            schema,
        });
        true
    }

    pub fn remove_item_effect(&mut self, item_id: ItemId, effect_id: EffectInstanceId) -> bool {
        let Some(item) = self.items.get_mut(&item_id).map(Arc::make_mut) else {
            return false;
        };
        let previous_len = item.effects.len();
        item.effects.retain(|effect| effect.id != effect_id);
        item.effects.len() != previous_len
    }

    pub fn move_item_effect(
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
}
