//! Version 4 → 5: restore source settings to ordinary authored properties.
use crate::{
    persistence::ProjectError,
    plugin::PluginRegistry,
    property::PropertyValue,
    timeline::{Frame, FrameDuration, FrameRate, TimeMapping},
};
use serde_json::{Map, Value};

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Timing {
    start: Frame,
    duration: FrameDuration,
    source: Option<SourceClock>,
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct SourceClock {
    offset: f32,
    speed: f32,
}

pub(super) fn migrate(
    file: &mut Map<String, Value>,
    plugins: &PluginRegistry,
) -> Result<(), ProjectError> {
    let rate = crate::persistence::project::migration::frame_rate(file)?;
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
        let timing: Timing = serde_json::from_value(
            item.remove("timing")
                .ok_or_else(|| ProjectError::invalid_data("Missing clip timing"))?,
        )
        .map_err(|error| ProjectError::invalid_format("Invalid clip timing", error))?;
        item.insert("start".into(), Value::from(timing.start.get()));
        if let Some(source) = timing.source {
            let kind = item
                .get("kind")
                .ok_or_else(|| ProjectError::invalid_data("Missing item kind"))?;
            let schema = plugins
                .item(
                    kind.get("plugin_id")
                        .and_then(Value::as_str)
                        .ok_or_else(|| ProjectError::invalid_data("Missing plugin ID"))?,
                    kind.get("item_id")
                        .and_then(Value::as_str)
                        .ok_or_else(|| ProjectError::invalid_data("Missing item ID"))?,
                )
                .ok_or_else(|| ProjectError::invalid_data("Playback plugin not found"))?;
            let playback = schema
                .playback()
                .ok_or_else(|| ProjectError::invalid_data("Playback declaration not found"))?;
            let mapping = TimeMapping::new(
                source.offset,
                (timing.duration.get() as f64 / rate.frames_per_second() * f64::from(source.speed))
                    as f32,
                source.speed,
            )
            .map_err(|error| ProjectError::invalid_data(error.to_string()))?;
            let duration = mapping.timeline_duration(rate);
            crate::persistence::project::migration::retime_animations(
                item,
                &schema,
                plugins,
                timing.duration,
                duration,
            )?;
            let properties = item
                .entry("properties")
                .or_insert_with(|| Value::Object(Map::new()))
                .as_object_mut()
                .ok_or_else(|| ProjectError::invalid_data("Invalid properties"))?;
            for (id, value) in [
                (&playback.source_start, mapping.source_offset()),
                (&playback.source_duration, mapping.source_span()),
                (&playback.playback_speed, mapping.speed()),
            ] {
                properties.insert(
                    id.clone(),
                    serde_json::to_value(PropertyValue::F32(value as f32)).map_err(|error| {
                        ProjectError::encode("Failed to migrate source settings", error)
                    })?,
                );
            }
        } else {
            item.insert("duration".into(), Value::from(timing.duration.get()));
        }
    }
    Ok(())
}
