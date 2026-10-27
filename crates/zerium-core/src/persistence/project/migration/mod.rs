//! Version dispatch and migration of project and scene file representations.
mod v1;
mod v2;
mod v3;
mod v4;
mod v5;
mod v6;
mod v7;
mod v8;
mod v9;

use serde::{Deserialize, de::DeserializeOwned};
use serde_json::{Map, Value};

use crate::{
    persistence::{
        DocumentKind, FORMAT_VERSION, ProjectError,
        items::{capture_animations, load_animations},
    },
    plugin::{ItemSchema, PluginRegistry},
    property::{PropertySchema, PropertyValue, PropertyValues},
    timeline::FrameDuration,
};

pub(in crate::persistence) fn decode<T: DeserializeOwned>(
    source: &str,
    plugins: &PluginRegistry,
) -> Result<T, ProjectError> {
    let header: Header = deserialize(source)?;
    // Scene files initially had their own version counter. Their definitions
    // match project formats 9 and 10; newer files share the document version.
    let format_version = match (header.kind, header.format_version) {
        (DocumentKind::Scene, 1) => 9,
        (DocumentKind::Scene, 2) => 10,
        (DocumentKind::Project, version @ 1..=FORMAT_VERSION)
        | (DocumentKind::Scene, version @ 10..=FORMAT_VERSION) => version,
        (_, version) => {
            return Err(ProjectError::unsupported_format(format!(
                "Unsupported file format (version {version})"
            )));
        }
    };
    match format_version {
        FORMAT_VERSION if header.format_version == FORMAT_VERSION => deserialize(source),
        _ => {
            let mut file: Map<String, Value> = deserialize(source)?;
            file.remove("format_version");
            let mut migration = Migration {
                format_version,
                file,
                plugins,
            };
            if migration.format_version <= 1 {
                v1::migrate(&mut migration)?;
            }
            if migration.format_version <= 2 {
                v2::migrate(&mut migration)?;
            }
            if migration.format_version <= 3 {
                v3::migrate(&mut migration)?;
            }
            if migration.format_version <= 4 {
                v4::migrate(&mut migration)?;
            }
            if migration.format_version <= 5 {
                v5::migrate(&mut migration)?;
            }
            if migration.format_version <= 6 {
                v6::migrate(&mut migration)?;
            }
            if migration.format_version <= 7 {
                v7::migrate(&mut migration)?;
            }
            if migration.format_version <= 8 {
                v8::migrate(&mut migration)?;
            }
            if migration.format_version <= 9 {
                v9::migrate(&mut migration)?;
            }
            migration.file.insert(
                "format_version".into(),
                Value::from(migration.format_version),
            );
            serde_json::from_value(Value::Object(migration.file)).map_err(invalid_format)
        }
    }
}

/// Migration state; the version advances with the document, including steps that
/// skip an intermediate format. Serialize it back into the file only at the end.
struct Migration<'a> {
    format_version: u32,
    file: Map<String, Value>,
    plugins: &'a PluginRegistry,
}

#[derive(Deserialize)]
struct Header {
    // Existing project formats have no document type marker.
    #[serde(default, rename = "type")]
    kind: DocumentKind,
    format_version: u32,
}

fn deserialize<'de, T: Deserialize<'de>>(source: &'de str) -> Result<T, ProjectError> {
    serde_json::from_str(source).map_err(invalid_format)
}

fn invalid_format(error: serde_json::Error) -> ProjectError {
    ProjectError::invalid_format(format!("Invalid Zerium file format: {error}"), error)
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
