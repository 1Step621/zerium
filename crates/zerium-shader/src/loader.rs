//! Plugin bundles backed by WESL module resolvers.

use std::{collections::BTreeMap, fs, path::Path, sync::Arc};

use wesl::resolver::{FileResolver, Resolver};
use zerium_core::plugin::{PluginError, PluginManifest, PluginRegistry};

use crate::{
    generate::{
        self, GENERATED_DIR, MANIFEST_FINGERPRINT,
        contract::{property_module, shader_contracts},
    },
    source::{EmbeddedResolver, ShaderSources, require_module, require_shader_modules},
};

/// A plugin manifest paired with its WESL source resolver.
pub struct PluginBundle {
    manifest: PluginManifest,
    sources: ShaderSources,
}

/// Shader sources and the corresponding core editing catalog.
pub struct PluginLibrary {
    registry: Arc<PluginRegistry>,
    sources: BTreeMap<String, ShaderSources>,
}

impl PluginLibrary {
    pub fn new(bundles: impl IntoIterator<Item = PluginBundle>) -> Result<Self, PluginError> {
        let mut registry = PluginRegistry::default();
        let mut sources = BTreeMap::new();
        for bundle in bundles {
            let id = bundle.manifest.id().to_owned();
            registry.register(bundle.manifest)?;
            sources.insert(id, bundle.sources);
        }
        Ok(Self {
            registry: Arc::new(registry),
            sources,
        })
    }

    pub fn registry(&self) -> Arc<PluginRegistry> {
        Arc::clone(&self.registry)
    }

    pub(crate) fn sources(&self, plugin_id: &str) -> Option<&ShaderSources> {
        self.sources.get(plugin_id)
    }
}

fn plugin_from_resolver(
    manifest: PluginManifest,
    fingerprint: &str,
    resolver: impl Resolver + Send + Sync + 'static,
    stale_message: impl Into<String>,
) -> Result<PluginBundle, PluginError> {
    let contracts = shader_contracts(&manifest)?;
    if fingerprint.trim() != generate::fingerprint(&generate::generated_files(&contracts)) {
        return Err(PluginError::invalid_definition(stale_message));
    }
    require_module(&resolver, "generated::host::util")?;
    for (entity, contract) in &contracts {
        for &kind in &contract.kinds {
            let module = property_module(entity, kind);
            resolver.resolve_source(&module).map_err(|error| {
                PluginError::missing_asset(format!(
                    "entity '{entity}' has no generated WESL module '{module}': {error}; run `zerium plugin generate`"
                ))
            })?;
        }
    }
    require_shader_modules(&manifest, &resolver)?;
    Ok(PluginBundle {
        manifest,
        sources: ShaderSources::new(resolver),
    })
}

pub fn load_filesystem_plugin(root: &Path) -> Result<PluginBundle, PluginError> {
    let manifest = read_manifest(root)?;
    let fingerprint_path = root.join(GENERATED_DIR).join(MANIFEST_FINGERPRINT);
    let fingerprint = read_asset(&fingerprint_path)?;
    plugin_from_resolver(
        manifest,
        &fingerprint,
        FileResolver::new(root),
        "generated WESL is stale; run `zerium plugin generate`",
    )
}

pub(crate) fn read_manifest(root: &Path) -> Result<PluginManifest, PluginError> {
    let manifest_path = root.join("plugin.json");
    PluginManifest::from_json(&read_asset(&manifest_path)?)
}

fn read_asset(path: &Path) -> Result<String, PluginError> {
    fs::read_to_string(path).map_err(|error| {
        PluginError::missing_asset(format!("cannot read '{}': {error}", path.display()))
    })
}

/// Loads an embedded bundle without enumerating or preloading its shader files.
/// The callback receives relative asset names such as `math/color.wesl`.
pub fn load_embedded_plugin(
    manifest: PluginManifest,
    fingerprint: &str,
    read_source: impl Fn(&str) -> Result<String, PluginError> + Send + Sync + 'static,
    stale_message: impl Into<String>,
) -> Result<PluginBundle, PluginError> {
    plugin_from_resolver(
        manifest,
        fingerprint,
        EmbeddedResolver::new(read_source),
        stale_message,
    )
}
