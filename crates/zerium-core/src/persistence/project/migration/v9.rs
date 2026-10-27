//! Version 9 → 10: persistent visibility for layers, items, and effects.
use super::Migration;

use crate::persistence::ProjectError;
use serde_json::Value;

pub(super) fn migrate(migration: &mut Migration<'_>) -> Result<(), ProjectError> {
    let scenes = migration
        .file
        .get_mut("scenes")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| ProjectError::invalid_data("Invalid scenes"))?;
    for scene in scenes {
        let scene = scene
            .as_object_mut()
            .ok_or_else(|| ProjectError::invalid_data("Invalid scene"))?;
        scene.insert("hidden_layers".into(), Value::Array(Vec::new()));
        let items = scene
            .get_mut("items")
            .and_then(Value::as_array_mut)
            .ok_or_else(|| ProjectError::invalid_data("Invalid scene items"))?;
        for item in items {
            let item = item
                .as_object_mut()
                .ok_or_else(|| ProjectError::invalid_data("Invalid item"))?;
            item.insert("hidden".into(), Value::Bool(false));
            let effects = item
                .get_mut("effects")
                .and_then(Value::as_array_mut)
                .ok_or_else(|| ProjectError::invalid_data("Invalid effects"))?;
            for effect in effects {
                effect
                    .as_object_mut()
                    .ok_or_else(|| ProjectError::invalid_data("Invalid effect"))?
                    .insert("hidden".into(), Value::Bool(false));
            }
        }
    }
    migration.format_version = 10;
    Ok(())
}
