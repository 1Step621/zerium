//! Plugin asset loading and generated-interface compatibility checks.

use crate::{shader_contract_fingerprint, shader_contracts};
use std::{collections::BTreeMap, fs, path::Path};
use zerium_core::plugin::{Plugin, PluginError, PluginManifest};

pub fn plugin_from_parts(
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

pub fn load_filesystem_plugin(root: &Path) -> Result<Plugin, PluginError> {
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
    plugin_from_parts(
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
