//! The runtime's bundled plugin provider and single-bundle loader.

use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
    sync::{Arc, LazyLock},
};

use rust_embed::RustEmbed;

use crate::domain::plugin::{Plugin, PluginError, PluginManifest, PluginRegistry};

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
                (!generated.contains('/')).then(|| format!("package::generated::{generated}"))?
            } else {
                (!relative.contains('/')).then(|| format!("package::{relative}"))?
            };
            Some((module, path))
        })
        .map(|(module, path)| bundled_text(&path).map(|contents| (module, contents)))
        .collect::<Result<BTreeMap<_, _>, _>>()?;
    from_parts(
        manifest,
        &manifest_source,
        &fingerprint,
        modules,
        format!(
            "bundled plugin '{directory}' has stale generated WESL; run `zerium plugin generate`"
        ),
    )
}

fn from_parts(
    manifest: PluginManifest,
    manifest_source: &str,
    fingerprint: &str,
    modules: BTreeMap<String, String>,
    stale_message: impl Into<String>,
) -> Result<Plugin, PluginError> {
    if fingerprint.trim() != manifest_fingerprint(manifest_source) {
        return Err(PluginError::invalid_definition(stale_message));
    }
    if !modules.contains_key("package::generated::util") {
        return Err(PluginError::missing_asset(
            "plugin has no generated WESL modules",
        ));
    }
    for module in manifest.shader_modules() {
        if !modules.contains_key(&format!("package::{module}")) {
            return Err(PluginError::missing_asset(format!(
                "shader module '{module}' has no {module}.wesl file"
            )));
        }
    }
    Ok(Plugin::new(manifest, modules))
}

pub(crate) fn manifest_fingerprint(source: &str) -> String {
    // Bump when the generated interface format or bundled templates change.
    const GENERATED_INTERFACE_REVISION: &[u8] = b"zerium-plugin-interface-1\0";
    let hash = GENERATED_INTERFACE_REVISION
        .iter()
        .copied()
        .chain(source.bytes())
        .fold(0xcbf2_9ce4_8422_2325_u64, |hash, byte| {
            (hash ^ u64::from(byte)).wrapping_mul(0x0000_0100_0000_01b3)
        });
    format!("{hash:016x}")
}

pub(crate) fn load_filesystem_plugin(root: &Path) -> Result<Plugin, PluginError> {
    let manifest_path = root.join("plugin.json");
    let manifest_source = fs::read_to_string(&manifest_path).map_err(|error| {
        PluginError::missing_asset(format!(
            "cannot read '{}': {error}",
            manifest_path.display()
        ))
    })?;
    let fingerprint_path = root.join("generated/manifest.fingerprint");
    let fingerprint = fs::read_to_string(&fingerprint_path).map_err(|error| {
        PluginError::missing_asset(format!(
            "cannot read '{}': {error}",
            fingerprint_path.display()
        ))
    })?;
    let manifest = PluginManifest::from_json(&manifest_source)?;
    let generated = root.join("generated");
    let mut modules = BTreeMap::new();
    for (directory, namespace) in [
        (root, "package::"),
        (generated.as_path(), "package::generated::"),
    ] {
        for entry in fs::read_dir(directory).map_err(|error| {
            PluginError::missing_asset(format!(
                "cannot read WESL directory '{}': {error}",
                directory.display()
            ))
        })? {
            let path = entry
                .map_err(|error| PluginError::missing_asset(error.to_string()))?
                .path();
            if path.extension().and_then(|extension| extension.to_str()) != Some("wesl") {
                continue;
            }
            let name = path
                .file_stem()
                .and_then(|name| name.to_str())
                .ok_or_else(|| {
                    PluginError::missing_asset(format!(
                        "WESL file '{}' has no valid module name",
                        path.display()
                    ))
                })?;
            let source = fs::read_to_string(&path).map_err(|error| {
                PluginError::missing_asset(format!("cannot read '{}': {error}", path.display()))
            })?;
            modules.insert(format!("{namespace}{name}"), source);
        }
    }
    from_parts(
        manifest,
        &manifest_source,
        &fingerprint,
        modules,
        "generated WESL is stale; run `zerium plugin generate`",
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
