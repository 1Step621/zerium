//! A validated manifest and the shader assets loaded for it.
//!
//! Executable shader validity remains a rendering responsibility.

use std::collections::BTreeMap;

use super::PluginManifest;

#[derive(Clone, Debug)]
pub struct Plugin {
    manifest: PluginManifest,
    modules: BTreeMap<String, String>,
}

impl Plugin {
    pub fn new(manifest: PluginManifest, modules: BTreeMap<String, String>) -> Self {
        Self { manifest, modules }
    }

    pub fn manifest(&self) -> &PluginManifest {
        &self.manifest
    }

    pub fn modules(&self) -> &BTreeMap<String, String> {
        &self.modules
    }
}
