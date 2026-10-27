//! Version 8 → 9: GIF export with looping enabled by default.
use super::Migration;

use crate::persistence::ProjectError;
use serde_json::Value;

pub(super) fn migrate(migration: &mut Migration<'_>) -> Result<(), ProjectError> {
    let settings = migration
        .file
        .get_mut("export_settings")
        .and_then(Value::as_object_mut)
        .ok_or_else(|| ProjectError::invalid_data("Invalid export settings"))?;
    settings.insert("gif_loop".into(), Value::Bool(true));
    migration.format_version = 9;
    Ok(())
}
