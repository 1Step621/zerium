//! Import routing shared by the explorer and operating-system file drops.
use std::path::{Path, PathBuf};

use zerium_core::plugin::PluginRegistry;

#[derive(Clone)]
pub(crate) struct FileImportTarget {
    pub(crate) plugin_id: String,
    pub(crate) item_id: String,
    pub(crate) property_id: String,
}

impl FileImportTarget {
    pub(crate) fn for_path(path: &Path, plugins: &PluginRegistry) -> Option<Self> {
        plugins.items().find_map(|(plugin_id, item)| {
            item.file_properties().find_map(|input| {
                input
                    .configuration_ui(None)
                    .matches_file(path)
                    .then(|| Self {
                        plugin_id: plugin_id.to_owned(),
                        item_id: item.id().to_owned(),
                        property_id: input.id().to_owned(),
                    })
            })
        })
    }
}

#[derive(Clone)]
pub(crate) struct FileImport {
    pub(crate) path: PathBuf,
    pub(crate) target: FileImportTarget,
}
