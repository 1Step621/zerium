//! Version 3 → 5: keep authored playback properties and discard derived state.
use crate::{
    persistence::ProjectError,
    plugin::PluginRegistry,
    property::{PropertyValue, PropertyValues},
    timeline::{FrameDuration, FrameRate},
};
use serde_json::{Map, Value};

pub(super) fn migrate(
    file: &mut Map<String, Value>,
    plugins: &PluginRegistry,
) -> Result<(), ProjectError> {
    let rate = super::frame_rate(file)?;
    file.remove("media_cache");
    items(file.get_mut("items"), plugins, rate)?;
    if let Some(scenes) = file.get_mut("scenes").and_then(Value::as_array_mut) {
        for scene in scenes {
            items(scene.get_mut("items"), plugins, rate)?;
        }
    }
    file.insert("format_version".into(), Value::from(5));
    Ok(())
}

fn items(
    value: Option<&mut Value>,
    plugins: &PluginRegistry,
    rate: FrameRate,
) -> Result<(), ProjectError> {
    for item in value
        .and_then(Value::as_array_mut)
        .ok_or_else(|| ProjectError::invalid_data("Missing items"))?
    {
        let item = item
            .as_object_mut()
            .ok_or_else(|| ProjectError::invalid_data("Invalid item"))?;
        let old_duration = item
            .get("duration")
            .and_then(Value::as_u64)
            .and_then(FrameDuration::new)
            .ok_or_else(|| ProjectError::invalid_data("Invalid item duration"))?;
        let kind = item
            .get("kind")
            .ok_or_else(|| ProjectError::invalid_data("Missing item kind"))?;
        if kind.get("type").and_then(Value::as_str) == Some("plugin") {
            let schema = plugins
                .item(
                    kind.get("plugin_id")
                        .and_then(Value::as_str)
                        .ok_or_else(|| ProjectError::invalid_data("Missing plugin ID"))?,
                    kind.get("item_id")
                        .and_then(Value::as_str)
                        .ok_or_else(|| ProjectError::invalid_data("Missing item ID"))?,
                )
                .ok_or_else(|| ProjectError::invalid_data("Item plugin not found"))?;
            if let Some(playback) = schema.playback() {
                let mut values = PropertyValues::from_properties(schema.properties());
                for id in playback.property_ids() {
                    if let Some(value) = item.get("properties").and_then(|values| values.get(id)) {
                        let value: PropertyValue =
                            serde_json::from_value(value.clone()).map_err(|error| {
                                ProjectError::invalid_format("Invalid playback property", error)
                            })?;
                        values
                            .set(schema.property(id).expect("playback property"), value)
                            .map_err(|error| ProjectError::invalid_data(error.to_string()))?;
                    }
                }
                let duration = playback
                    .mapping(&values)
                    .map_err(|error| ProjectError::invalid_data(error.to_string()))?
                    .timeline_duration(rate);
                super::retime_animations(item, &schema, plugins, old_duration, duration)?;
                item.remove("duration");
            }
        }
    }
    Ok(())
}
