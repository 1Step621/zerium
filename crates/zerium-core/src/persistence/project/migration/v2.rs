//! Version 2 → 3: discard path-level media revisions for rediscovery.
use super::Migration;

use crate::persistence::ProjectError;
use serde_json::Value;

pub(super) fn migrate(migration: &mut Migration<'_>) -> Result<(), ProjectError> {
    let file = &mut migration.file;
    // Version 3 stores revisions per reader/target observation. Preserve the
    // authored data and let the runtime rebuild this derived cache.
    if let Some(cache) = file.get_mut("media_cache") {
        *cache = Value::Array(Vec::new());
    }
    migration.format_version = 3;
    Ok(())
}
