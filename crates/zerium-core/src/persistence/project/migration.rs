//! Version dispatch and migration into the current project representation.
mod v1;
mod v2;
mod v3;
mod v4;

use serde::Deserialize;
use serde_json::{Map, Value};

use super::{FORMAT_VERSION, ProjectError, ProjectFile};
use crate::{
    persistence::items::{capture_animations, load_animations},
    plugin::{ItemSchema, PluginRegistry},
    property::{PropertySchema, PropertyValue, PropertyValues},
    timeline::FrameDuration,
};

pub(super) fn decode(
    source: &str,
    plugins: &crate::plugin::PluginRegistry,
) -> Result<ProjectFile, ProjectError> {
    let header: ProjectHeader = deserialize(source)?;
    match header.format_version {
        1..=4 => {
            let mut file: Map<String, Value> = deserialize(source)?;
            if header.format_version == 1 {
                v1::migrate(&mut file)?;
            }
            if header.format_version <= 2 {
                v2::migrate(&mut file);
            }
            if header.format_version <= 3 {
                v3::migrate(&mut file, plugins)?;
            } else {
                v4::migrate(&mut file, plugins)?;
            }
            serde_json::from_value(Value::Object(file)).map_err(invalid_format)
        }
        FORMAT_VERSION => deserialize(source),
        version => Err(ProjectError::unsupported_format(format!(
            "Unsupported project format (version {version})"
        ))),
    }
}

#[derive(Deserialize)]
struct ProjectHeader {
    format_version: u32,
}

fn deserialize<'de, T: Deserialize<'de>>(source: &'de str) -> Result<T, ProjectError> {
    serde_json::from_str(source).map_err(invalid_format)
}

fn invalid_format(error: serde_json::Error) -> ProjectError {
    ProjectError::invalid_format(format!("Invalid project file format: {error}"), error)
}

fn frame_rate(file: &Map<String, Value>) -> Result<crate::timeline::FrameRate, ProjectError> {
    let rate: [u32; 2] = serde_json::from_value(
        file.get("frame_rate")
            .cloned()
            .ok_or_else(|| ProjectError::invalid_data("Missing frame rate"))?,
    )
    .map_err(|error| ProjectError::invalid_format("Invalid frame rate", error))?;
    crate::timeline::FrameRate::new(rate[0], rate[1])
        .ok_or_else(|| ProjectError::invalid_data("Invalid frame rate"))
}

/// Fit legacy animation intervals to the source-derived clip duration.
fn retime_animations(
    item: &mut Map<String, Value>,
    schema: &ItemSchema,
    plugins: &PluginRegistry,
    old: FrameDuration,
    duration: FrameDuration,
) -> Result<(), ProjectError> {
    if old == duration {
        return Ok(());
    }
    retime_owner_animations(item, schema.properties(), old, duration)?;
    if let Some(effects) = item.get_mut("effects").and_then(Value::as_array_mut) {
        for effect in effects {
            let effect = effect
                .as_object_mut()
                .ok_or_else(|| ProjectError::invalid_data("Invalid effect"))?;
            let schema = plugins
                .effect(
                    effect
                        .get("plugin_id")
                        .and_then(Value::as_str)
                        .ok_or_else(|| ProjectError::invalid_data("Missing effect plugin ID"))?,
                    effect
                        .get("effect_id")
                        .and_then(Value::as_str)
                        .ok_or_else(|| ProjectError::invalid_data("Missing effect ID"))?,
                )
                .ok_or_else(|| ProjectError::invalid_data("Effect plugin not found"))?;
            retime_owner_animations(effect, schema.properties(), old, duration)?;
        }
    }
    Ok(())
}

fn retime_owner_animations(
    owner: &mut Map<String, Value>,
    schema: &[PropertySchema],
    old: FrameDuration,
    duration: FrameDuration,
) -> Result<(), ProjectError> {
    let mut values = PropertyValues::from_properties(schema);
    if let Some(properties) = owner.get("properties").and_then(Value::as_object) {
        for (id, value) in properties {
            let property = schema
                .iter()
                .find(|property| property.id() == id)
                .ok_or_else(|| ProjectError::invalid_data("Unknown property"))?;
            let value: PropertyValue = serde_json::from_value(value.clone())
                .map_err(|error| ProjectError::invalid_format("Invalid property", error))?;
            values
                .set(property, value)
                .map_err(|error| ProjectError::invalid_data(error.to_string()))?;
        }
    }
    let tracks = serde_json::from_value(
        owner
            .remove("animations")
            .ok_or_else(|| ProjectError::invalid_data("Missing animations"))?,
    )
    .map_err(|error| ProjectError::invalid_format("Invalid animations", error))?;
    let mut animations = load_animations(schema, &values, tracks, old)?;
    animations.fit_duration(duration);
    owner.insert(
        "animations".into(),
        serde_json::to_value(capture_animations(&animations))
            .map_err(|error| ProjectError::encode("Failed to migrate animations", error))?,
    );
    Ok(())
}
