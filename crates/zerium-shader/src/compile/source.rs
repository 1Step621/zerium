//! Plugin bundles backed by WESL module resolvers.

use std::{borrow::Cow, collections::BTreeMap, fs, path::Path, sync::Arc};

use wesl::{
    resolver::{FileResolver, Resolver, VirtualResolver},
    syntax::ModulePath,
};
use zerium_core::plugin::{PluginError, PluginManifest, PluginRegistry};

use crate::generate::{self, GENERATED_DIR, MANIFEST_FINGERPRINT, contract::shader_contracts};

/// A plugin manifest paired with its WESL source resolver.
pub struct PluginBundle {
    manifest: PluginManifest,
    resolver: Box<dyn Resolver + Send + Sync>,
}

/// Shader sources and the corresponding core editing catalog.
pub struct PluginLibrary {
    registry: Arc<PluginRegistry>,
    resolvers: BTreeMap<String, Box<dyn Resolver + Send + Sync>>,
}

impl PluginLibrary {
    pub fn new(bundles: impl IntoIterator<Item = PluginBundle>) -> Result<Self, PluginError> {
        let mut registry = PluginRegistry::default();
        let mut resolvers = BTreeMap::new();
        for bundle in bundles {
            let id = bundle.manifest.id().to_owned();
            registry.register(bundle.manifest)?;
            resolvers.insert(id, bundle.resolver);
        }
        Ok(Self {
            registry: Arc::new(registry),
            resolvers,
        })
    }

    pub fn registry(&self) -> Arc<PluginRegistry> {
        Arc::clone(&self.registry)
    }

    pub(crate) fn resolver(&self, plugin_id: &str) -> Option<&(dyn Resolver + Send + Sync)> {
        self.resolvers.get(plugin_id).map(Box::as_ref)
    }
}

fn plugin_from_resolver(
    manifest: PluginManifest,
    fingerprint: &str,
    resolver: impl Resolver + Send + Sync + 'static,
    stale_message: impl Into<String>,
) -> Result<PluginBundle, PluginError> {
    let contracts = shader_contracts(&manifest)?;
    let modules = generate::generated_modules(&contracts);
    if fingerprint.trim() != generate::fingerprint(&modules) {
        return Err(PluginError::invalid_definition(stale_message));
    }

    for module in &modules {
        require_module(&resolver, &module.path)?;
    }
    require_shader_modules(&manifest, &resolver)?;
    Ok(PluginBundle {
        manifest,
        resolver: Box::new(resolver),
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

/// Loads an embedded bundle from module paths and their WESL sources.
/// Paths are absolute within the plugin, such as `package::math::color`.
pub fn load_embedded_plugin(
    manifest: PluginManifest,
    fingerprint: &str,
    modules: impl IntoIterator<Item = (ModulePath, Cow<'static, str>)>,
    stale_message: impl Into<String>,
) -> Result<PluginBundle, PluginError> {
    let mut resolver = VirtualResolver::new();
    for (path, source) in modules {
        resolver.add_module(path, source);
    }
    plugin_from_resolver(manifest, fingerprint, resolver, stale_message)
}

pub(crate) fn require_shader_modules(
    manifest: &PluginManifest,
    resolver: &dyn Resolver,
) -> Result<(), PluginError> {
    for module in manifest.shader_modules() {
        let path = module_path(module)?;
        require_module(resolver, &path)?;
    }
    Ok(())
}

fn require_module(resolver: &dyn Resolver, path: &ModulePath) -> Result<(), PluginError> {
    resolver.resolve_source(path).map(|_| ()).map_err(|error| {
        PluginError::missing_asset(format!("shader module '{path}' was not found: {error}"))
    })
}

pub(crate) fn module_path(module: &str) -> Result<ModulePath, PluginError> {
    format!("package::{module}").parse().map_err(|error| {
        PluginError::invalid_definition(format!("invalid WESL module path '{module}': {error}"))
    })
}
