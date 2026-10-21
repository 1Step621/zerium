//! Media initialization and collision-aware document placement.

use std::sync::Arc;

use crate::media::ImportedFile;
use crate::property::PropertyValue;
use crate::timeline::item::{set_size_values, size_values};
use crate::timeline::{ItemId, TimeMapping};

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
}
