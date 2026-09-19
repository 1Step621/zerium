//! A validated manifest and the shader assets loaded for it.
//!
//! Executable shader validity remains a rendering responsibility.

use std::collections::BTreeMap;

use super::PluginManifest;

#[derive(Clone, Debug)]
pub(crate) struct Plugin {
    manifest: PluginManifest,
    shader_sources: BTreeMap<String, String>,
    wesl_modules: BTreeMap<String, String>,
}

impl Plugin {
    pub(crate) fn new(
        manifest: PluginManifest,
        shader_sources: BTreeMap<String, String>,
        wesl_modules: BTreeMap<String, String>,
    ) -> Self {
        Self {
            manifest,
            shader_sources,
            wesl_modules,
        }
    }

    pub(crate) fn manifest(&self) -> &PluginManifest {
        &self.manifest
    }

    pub(crate) fn shader_source(&self, source: &str) -> Option<&str> {
        self.shader_sources.get(source).map(String::as_str)
    }

    pub(crate) fn wesl_modules(&self) -> &BTreeMap<String, String> {
        &self.wesl_modules
    }
}
