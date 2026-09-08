//! Plugin manifest parsing and top-level identity validation.

use std::sync::Arc;

use serde::Deserialize;

use super::{
    EffectSchema, ItemSchema, PluginError, identifier::validate_logical_id,
    validation::validate_unique_ids,
};

const SUPPORTED_API_VERSION: u32 = 1;

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct PluginManifest {
    pub(crate) api_version: u32,
    pub(crate) id: String,
    items: Vec<Arc<ItemSchema>>,
    effects: Vec<Arc<EffectSchema>>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawPluginManifest {
    #[serde(rename = "$schema")]
    schema: Option<String>,
    api_version: u32,
    id: String,
    #[serde(default)]
    items: Vec<ItemSchema>,
    #[serde(default)]
    effects: Vec<EffectSchema>,
}

impl PluginManifest {
    pub(crate) fn from_json(source: &str) -> Result<Self, PluginError> {
        let raw = serde_json::from_str::<RawPluginManifest>(source).map_err(|error| {
            PluginError::invalid_manifest(format!("invalid plugin manifest: {error}"), error)
        })?;
        let _schema = raw.schema;
        let manifest = Self {
            api_version: raw.api_version,
            id: raw.id,
            items: raw.items.into_iter().map(Arc::new).collect(),
            effects: raw.effects.into_iter().map(Arc::new).collect(),
        };
        manifest.validate()?;
        Ok(manifest)
    }

    fn validate(&self) -> Result<(), PluginError> {
        if self.api_version != SUPPORTED_API_VERSION {
            return Err(PluginError::invalid_definition(format!(
                "unsupported plugin API version {}; expected {SUPPORTED_API_VERSION}",
                self.api_version
            )));
        }
        validate_logical_id("plugin", &self.id)?;
        if self.items.is_empty() && self.effects.is_empty() {
            return Err(PluginError::invalid_definition(
                "plugin must define at least one item or effect",
            ));
        }

        validate_unique_ids("item", self.items.iter().map(|schema| schema.id()))?;
        validate_unique_ids("effect", self.effects.iter().map(|schema| schema.id()))?;
        Ok(())
    }

    pub(crate) fn id(&self) -> &str {
        &self.id
    }

    pub(crate) fn items(&self) -> impl ExactSizeIterator<Item = &ItemSchema> {
        self.items.iter().map(Arc::as_ref)
    }

    pub(super) fn item_schemas(&self) -> &[Arc<ItemSchema>] {
        &self.items
    }

    pub(super) fn effect_schemas(&self) -> &[Arc<EffectSchema>] {
        &self.effects
    }

    pub(crate) fn shader_sources(&self) -> impl Iterator<Item = &str> {
        let item_sources = self
            .items
            .iter()
            .filter_map(|schema| schema.visual_shader())
            .map(super::ShaderSchema::source);
        let effect_sources = self
            .effects
            .iter()
            .flat_map(|effect| effect.passes().iter().map(|pass| pass.shader_source()));
        item_sources.chain(effect_sources)
    }
}
