//! Version 7 → 8: automatic encoder selection.
use super::Migration;

use crate::persistence::ProjectError;
use serde_json::Value;

pub(super) fn migrate(migration: &mut Migration<'_>) -> Result<(), ProjectError> {
    let settings = migration
        .file
        .get_mut("export_settings")
        .and_then(Value::as_object_mut)
        .ok_or_else(|| ProjectError::invalid_data("Invalid export settings"))?;
    settings.remove("encoder");
    migration.format_version = 8;
    Ok(())
}
