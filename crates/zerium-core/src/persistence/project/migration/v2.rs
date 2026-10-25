//! Version 2 → 3: discard path-level media revisions for rediscovery.
use serde_json::{Map, Value};

pub(super) fn migrate(file: &mut Map<String, Value>) {
    // Version 3 stores revisions per reader/target observation. Preserve the
    // authored data and let the runtime rebuild this derived cache.
    if let Some(cache) = file.get_mut("media_cache") {
        *cache = Value::Array(Vec::new());
    }
    file.insert("format_version".to_owned(), Value::from(3));
}
