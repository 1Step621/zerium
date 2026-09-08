//! A manifest paired with all referenced assets.
//!
//! `Plugin` guarantees structural/semantic manifest validity and complete UTF-8 asset resolution.
//! Executable WGSL validity is deliberately a later responsibility because it requires composing
//! renderer-owned interfaces with the schema-owned parameter ABI.

use std::collections::BTreeMap;

use super::{PluginError, PluginManifest};

#[derive(Clone, Debug)]
pub(crate) struct Plugin {
    manifest: PluginManifest,
    shader_sources: BTreeMap<String, String>,
}

impl Plugin {
    pub(crate) fn from_loader(
        manifest_source: &str,
        mut load_shader: impl FnMut(&str) -> Result<String, PluginError>,
    ) -> Result<Self, PluginError> {
        let manifest = PluginManifest::from_json(manifest_source)?;
        let mut shader_sources = BTreeMap::new();
        for source in manifest.shader_sources() {
            if !shader_sources.contains_key(source) {
                shader_sources.insert(source.to_owned(), load_shader(source)?);
            }
        }
        Ok(Self {
            manifest,
            shader_sources,
        })
    }

    pub(crate) fn manifest(&self) -> &PluginManifest {
        &self.manifest
    }

    pub(crate) fn shader_source(&self, source: &str) -> Option<&str> {
        self.shader_sources.get(source).map(String::as_str)
    }
}
