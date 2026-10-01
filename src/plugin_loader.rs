//! The runtime's bundled plugin provider and single-bundle loader.

use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
    sync::{Arc, LazyLock},
};

use rust_embed::RustEmbed;

use crate::domain::plugin::{
    Plugin, PluginError, PluginManifest, PluginRegistry, shader_contract_fingerprint,
    shader_contracts,
};

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
                format!("package::generated::{}", generated.replace('/', "::"))
            } else {
                (!relative.contains('/')).then(|| format!("package::{relative}"))?
            };
            Some((module, path))
        })
        .map(|(module, path)| bundled_text(&path).map(|contents| (module, contents)))
        .collect::<Result<BTreeMap<_, _>, _>>()?;
    from_parts(
        manifest,
        &fingerprint,
        modules,
        format!(
            "bundled plugin '{directory}' has stale generated WESL; run `zerium plugin generate`"
        ),
    )
}

fn from_parts(
    manifest: PluginManifest,
    fingerprint: &str,
    modules: BTreeMap<String, String>,
    stale_message: impl Into<String>,
) -> Result<Plugin, PluginError> {
    let contracts = shader_contracts(&manifest)?;
    if fingerprint.trim() != shader_contract_fingerprint(&contracts)? {
        return Err(PluginError::invalid_definition(stale_message));
    }
    if !modules.contains_key("package::generated::host::util") {
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
    for entity in contracts.keys() {
        if !modules.contains_key(&format!("package::generated::{entity}")) {
            return Err(PluginError::missing_asset(format!(
                "entity '{entity}' has no generated/{entity}.wesl file; run `zerium plugin generate`"
            )));
        }
    }
    Ok(Plugin::new(manifest, modules))
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
    read_wesl_directory(root, "package::", false, &mut modules)?;
    read_wesl_directory(&generated, "package::generated::", true, &mut modules)?;
    from_parts(
        manifest,
        &fingerprint,
        modules,
        "generated WESL is stale; run `zerium plugin generate`",
    )
}

fn read_wesl_directory(
    directory: &Path,
    namespace: &str,
    recursive: bool,
    modules: &mut BTreeMap<String, String>,
) -> Result<(), PluginError> {
    let entries = fs::read_dir(directory).map_err(|error| {
        PluginError::missing_asset(format!(
            "cannot read WESL directory '{}': {error}",
            directory.display()
        ))
    })?;
    for entry in entries {
        let entry = entry.map_err(|error| PluginError::missing_asset(error.to_string()))?;
        let path = entry.path();
        let file_type = entry
            .file_type()
            .map_err(|error| PluginError::missing_asset(error.to_string()))?;
        if recursive && file_type.is_dir() {
            let name = entry.file_name();
            let name = name.to_str().ok_or_else(|| {
                PluginError::missing_asset(format!(
                    "WESL directory '{}' has no valid module name",
                    path.display()
                ))
            })?;
            read_wesl_directory(&path, &format!("{namespace}{name}::"), true, modules)?;
            continue;
        }
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
    Ok(())
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
