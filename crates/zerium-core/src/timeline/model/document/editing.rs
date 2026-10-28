//! Media initialization and collision-aware document placement.

use std::sync::Arc;

use crate::{
    media::ImportedFile,
    property::PropertyValue,
    timeline::{
        ItemId,
        model::item::{set_size_values, size_values},
    },
};

use super::TimelineDocument;

impl TimelineDocument {
    pub(in crate::timeline) fn import_item_file(
        &mut self,
        id: ItemId,
        imported: ImportedFile,
    ) -> bool {
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
        let assets = schema
            .media_sources()
            .filter(|source| source.file == imported.property_id)
            .filter_map(|source| {
                let mut asset = imported.file.asset(source.reader, source.input.target())?;
                source.apply_frame_rate(
                    &mut asset,
                    &item.properties.materialize(schema.properties()),
                )?;
                Some((source.input, asset))
            })
            .collect::<Vec<_>>();
        let primary = schema.playback().map(|playback| &playback.source);
        let dimensions = assets
            .iter()
            .filter(|(input, _)| primary.is_none_or(|primary| primary == input))
            .find_map(|(_, asset)| asset.kind.dimensions());
        let initial_duration = assets
            .iter()
            .find(|(input, asset)| primary == Some(input) && asset.kind.is_temporal())
            .map(|(_, asset)| asset.duration);
        if let Some(source_duration) = initial_duration
            && item
                .initialize_source(source_duration, self.frame_rate)
                .is_none()
        {
            return false;
        }
        let duration = item.duration(self.frame_rate);
        let Some(layer) = self.item_layers.get(&id).copied() else {
            return false;
        };
        let Some(start) = self.nearest_available_start(layer, item.start(), duration, Some(id))
        else {
            return false;
        };
        item.move_to(start);
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
        if item.validate_playback(self.frame_rate).is_err() {
            return false;
        }
        self.items.insert(id, Arc::new(item));
        #[cfg(debug_assertions)]
        self.assert_consistent();
        true
    }
}
