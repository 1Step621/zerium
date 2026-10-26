//! Version 6 → 7: project-owned export settings.
use super::Migration;

use crate::{persistence::ProjectError, timeline::ExportSettings};

pub(super) fn migrate(migration: &mut Migration<'_>) -> Result<(), ProjectError> {
    let file = &mut migration.file;
    file.insert(
        "export_settings".into(),
        serde_json::to_value(ExportSettings::default())
            .map_err(|error| ProjectError::invalid_format("Invalid export settings", error))?,
    );
    migration.format_version = 7;
    Ok(())
}
