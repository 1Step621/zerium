//! Import routing shared by file pickers, the explorer, and file drops.
use std::path::{Path, PathBuf};

use zerium_core::{
    plugin::PluginRegistry,
    timeline::{PropertyAddress, TimelineEditor},
};

#[derive(Clone)]
pub(crate) struct FileImportTarget {
    pub(crate) plugin_id: String,
    pub(crate) item_id: String,
    pub(crate) property_id: String,
}

impl FileImportTarget {
    /// The first media selection initializes an item's dimensions and duration.
    /// Effects and compound fields only replace their property value.
    pub(crate) fn for_property(address: &PropertyAddress, editor: &TimelineEditor) -> Option<Self> {
        if address.effect_id.is_some()
            || address.element_id.is_some()
            || address.scalar_index.is_some()
        {
            return None;
        }
        let item = editor.item(address.item_id)?;
        if editor.property_value(address)?.file().is_some() {
            return None;
        }
        let plugin_id = item.plugin_id()?;
        let schema = item.schema()?;
        schema
            .media_sources()
            .any(|source| source.file == address.property_id)
            .then(|| Self {
                plugin_id: plugin_id.to_owned(),
                item_id: schema.id().to_owned(),
                property_id: address.property_id.clone(),
            })
    }

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
