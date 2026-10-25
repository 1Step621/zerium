//! Declarative plugin contracts and their semantic validation.
use self::validation::{validate_logical_id, validate_unique_ids, validate_wgsl_identifier};
use super::{EffectSchema, ItemSchema, PluginError};
use serde::Deserialize;
use std::sync::Arc;

pub(super) mod bounds;
pub(super) mod capability;
pub(super) mod editor;
pub(super) mod effect;
pub(super) mod item;
pub(super) mod validation;

const SUPPORTED_API_VERSION: u32 = 1;

#[derive(Clone, Debug, PartialEq)]
pub struct PluginManifest {
    pub api_version: u32,
    pub id: String,
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
    pub fn from_json(source: &str) -> Result<Self, PluginError> {
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

    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn items(&self) -> impl ExactSizeIterator<Item = &ItemSchema> {
        self.items.iter().map(Arc::as_ref)
    }

    pub fn effects(&self) -> impl ExactSizeIterator<Item = &EffectSchema> {
        self.effects.iter().map(Arc::as_ref)
    }

    pub(super) fn item_schemas(&self) -> &[Arc<ItemSchema>] {
        &self.items
    }

    pub(super) fn effect_schemas(&self) -> &[Arc<EffectSchema>] {
        &self.effects
    }

    pub fn shader_modules(&self) -> impl Iterator<Item = &str> {
        let item_modules = self
            .items
            .iter()
            .filter_map(|schema| schema.render().map(|render| render.shader.module()));
        let effect_modules = self.effects.iter().flat_map(|effect| {
            effect
                .render()
                .passes
                .iter()
                .filter_map(|pass| pass.shader_module())
        });
        item_modules.chain(effect_modules)
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ShaderSchema {
    module: String,
    #[serde(default = "default_vertex_entry")]
    vertex_entry: String,
    #[serde(default = "default_fragment_entry")]
    fragment_entry: String,
}

impl ShaderSchema {
    pub fn module(&self) -> &str {
        &self.module
    }

    pub fn vertex_entry(&self) -> &str {
        &self.vertex_entry
    }

    pub fn fragment_entry(&self) -> &str {
        &self.fragment_entry
    }

    pub(super) fn validate(&self, owner_kind: &str, owner_id: &str) -> Result<(), PluginError> {
        validate_shader_module(owner_kind, owner_id, &self.module)?;
        validate_wgsl_identifier("vertex entry point", &self.vertex_entry)?;
        validate_wgsl_identifier("fragment entry point", &self.fragment_entry)?;
        Ok(())
    }
}

pub(super) fn validate_shader_module(
    owner_kind: &str,
    owner_id: &str,
    module: &str,
) -> Result<(), PluginError> {
    if module.split("::").next() == Some("generated") {
        return Err(PluginError::invalid_definition(format!(
            "{owner_kind} '{owner_id}' uses reserved shader module '{module}'"
        )));
    }

    for segment in module.split("::") {
        validate_wgsl_identifier("shader module path segment", segment)?;
    }
    Ok(())
}

fn default_vertex_entry() -> String {
    "vertex_main".into()
}

fn default_fragment_entry() -> String {
    "fragment_main".into()
}
