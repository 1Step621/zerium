//! The application's embedded plugin provider.

use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{Arc, LazyLock},
};

use rust_embed::RustEmbed;

use crate::domain::plugin::{Plugin, PluginError, PluginManifest, PluginRegistry};

#[derive(RustEmbed)]
#[folder = "../../plugins/"]
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
        .map(|directory| load_bundled_plugin(&directory))
        .collect::<Result<Vec<_>, _>>()?;
    PluginRegistry::new(bundles)
}

fn load_bundled_plugin(directory: &str) -> Result<Plugin, PluginError> {
    let manifest_source = bundled_text(&format!("{directory}/plugin.json"))?;
    let fingerprint = bundled_text(&format!("{directory}/generated/manifest.fingerprint"))?;
    let manifest = PluginManifest::from_json(&manifest_source)?;
    let plugin_prefix = format!("{directory}/");
    let modules = BundledPluginAssets::iter()
        .filter_map(|path| {
            let path = path.into_owned();
            let relative = path.strip_prefix(&plugin_prefix)?.strip_suffix(".wesl")?;
            let module = if let Some(generated) = relative.strip_prefix("generated/") {
                format!("package::generated::{}", generated.replace('/', "::"))
            } else {
                (!relative.contains('/')).then(|| format!("package::{relative}"))?
            };
            Some((module, path))
        })
        .map(|(module, path)| bundled_text(&path).map(|contents| (module, contents)))
        .collect::<Result<BTreeMap<_, _>, _>>()?;
    zerium_shader::plugin_from_parts(
        manifest,
        &fingerprint,
        modules,
        format!(
            "bundled plugin '{directory}' has stale generated WESL; run `zerium plugin generate`"
        ),
    )
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
