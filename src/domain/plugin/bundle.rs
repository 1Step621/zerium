//! A validated manifest and the shader assets loaded for it.
//!
//! Executable shader validity remains a rendering responsibility.

use std::collections::BTreeMap;

use super::PluginManifest;

#[derive(Clone, Debug)]
pub(crate) struct Plugin {
    manifest: PluginManifest,
    modules: BTreeMap<String, String>,
}

impl Plugin {
    pub(crate) fn new(manifest: PluginManifest, modules: BTreeMap<String, String>) -> Self {
        Self { manifest, modules }
    }

    pub(crate) fn manifest(&self) -> &PluginManifest {
        &self.manifest
    }

    pub(crate) fn modules(&self) -> &BTreeMap<String, String> {
        &self.modules
    }
}
