//! The application's embedded plugin provider.

use std::{
    collections::BTreeSet,
    sync::{Arc, LazyLock},
};

use rust_embed::RustEmbed;

use zerium_core::plugin::{PluginError, PluginManifest, PluginRegistry};
use zerium_shader::{ModulePath, PluginBundle, PluginLibrary};

#[derive(RustEmbed)]
#[folder = "../../plugins/"]
struct BundledPluginAssets;

static BUNDLED_PLUGINS: LazyLock<PluginLibrary> = LazyLock::new(|| {
    load_bundled_plugins()
        .expect("bundled plugin manifests and referenced assets must be resolvable")
});

/// Returns the immutable plugin catalog shipped with this application.
pub(crate) fn plugins() -> Arc<PluginRegistry> {
    BUNDLED_PLUGINS.registry()
}

pub(crate) fn library() -> &'static PluginLibrary {
    &BUNDLED_PLUGINS
}

pub(crate) fn validate_icons(plugins: &PluginRegistry) -> Result<(), PluginError> {
    let entries = plugins
        .items()
        .map(|(plugin, item)| (plugin, "item", item.id(), item.icon()))
        .chain(
            plugins
                .effects()
                .map(|(plugin, effect)| (plugin, "effect", effect.id(), effect.icon())),
        );
    for (plugin, kind, id, icon) in entries {
        if !crate::ui::icon_exists(icon) {
            return Err(PluginError::invalid_definition(format!(
                "plugin '{plugin}' {kind} '{id}' references unknown icon '{icon}'"
            )));
        }
    }
    Ok(())
}

fn load_bundled_plugins() -> Result<PluginLibrary, PluginError> {
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
    let library = PluginLibrary::new(bundles)?;
    validate_icons(&library.registry())?;
    Ok(library)
}

fn load_bundled_plugin(directory: &str) -> Result<PluginBundle, PluginError> {
    let manifest_source = bundled_text(&format!("{directory}/plugin.json"))?;
    let fingerprint = bundled_text(&format!("{directory}/generated/manifest.fingerprint"))?;
    let manifest = PluginManifest::from_json(&manifest_source)?;
    let prefix = format!("{directory}/");
    let modules = BundledPluginAssets::iter()
        .filter_map(|asset| {
            let name = asset.strip_prefix(&prefix)?.strip_suffix(".wesl")?;
            let mut path = ModulePath::new_root();
            path.components.extend(name.split('/').map(str::to_owned));

            Some(bundled_text(&asset).map(|source| (path, source.into())))
        })
        .collect::<Result<Vec<_>, _>>()?;

    zerium_shader::load_embedded_plugin(
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
