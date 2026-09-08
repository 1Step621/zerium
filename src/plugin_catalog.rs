//! Process-wide catalog of plugins bundled with the application.
//!
//! Asset transport belongs here rather than in `domain::plugin`: bundles and
//! registries stay independent from how their source files reach the process.

use std::{
    collections::BTreeSet,
    sync::{Arc, LazyLock},
};

use rust_embed::RustEmbed;

use crate::domain::plugin::{Plugin, PluginError, PluginRegistry};

#[derive(RustEmbed)]
#[folder = "plugins/"]
struct BundledPluginAssets;

static BUNDLED_PLUGINS: LazyLock<Arc<PluginRegistry>> = LazyLock::new(|| {
    Arc::new(
        load_bundled_plugins()
            .expect("bundled plugin manifests and referenced assets must be resolvable"),
    )
});

/// Returns the immutable plugin catalog shipped with this application.
pub(crate) fn plugins() -> Arc<PluginRegistry> {
    BUNDLED_PLUGINS.clone()
}

fn load_bundled_plugins() -> Result<PluginRegistry, PluginError> {
    let directories = BundledPluginAssets::iter()
        .filter_map(|path| path.strip_suffix("/plugin.json").map(str::to_owned))
        .collect::<BTreeSet<_>>();
    if directories.is_empty() {
        return Err(PluginError::invalid_definition(
            "no bundled plugins were found",
        ));
    }

    let bundles = directories
        .into_iter()
        .map(|directory| {
            let manifest_path = format!("{directory}/plugin.json");
            let manifest_source = bundled_text(&manifest_path)?;
            Plugin::from_loader(&manifest_source, |source| {
                bundled_text(&format!("{directory}/{source}"))
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    PluginRegistry::new(bundles)
}

fn bundled_text(path: &str) -> Result<String, PluginError> {
    let file = BundledPluginAssets::get(path).ok_or_else(|| {
        PluginError::missing_asset(format!("bundled plugin asset '{path}' was not found"))
    })?;
    String::from_utf8(file.data.into_owned()).map_err(|error| {
        PluginError::missing_asset(format!(
            "bundled plugin asset '{path}' is not UTF-8: {error}"
        ))
    })
}
