//! Item schemas and their runtime property ABI.

use crate::localized_text::LocalizedText;
use std::collections::HashSet;

use serde::{Deserialize, Deserializer, de::Error as _};

use super::CatalogCategory;
use super::OutputBoundsSchema;
use super::PluginError;
use super::abi::PropertyLayout;
use super::capability::{
    AudioCapability, Capability, TimeMappingProperties, validate_capabilities,
};
use super::editor::{EditorCapability, validate_editors};
use super::shader::ShaderSchema;
use super::validation::{validate_catalog_entry, validate_property_schemas};
use crate::property::PropertySchema;

#[derive(Clone, Debug, PartialEq)]
pub struct ItemSchema {
    id: String,
    label: LocalizedText,
    category: CatalogCategory,
    tags: Vec<String>,
    symbol: String,
    shader: Option<ShaderSchema>,
    vertex_count: u32,
    capabilities: Vec<Capability>,
    audio: Vec<AudioCapability>,
    editor: Vec<EditorCapability>,
    output_bounds: OutputBoundsSchema,
    properties: Vec<PropertySchema>,
    property_abi: PropertyLayout,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ItemSchemaDefinition {
    id: String,
    label: LocalizedText,
    category: CatalogCategory,
    #[serde(default)]
    tags: Vec<String>,
    symbol: String,
    #[serde(default)]
    shader: Option<ShaderSchema>,
    #[serde(default = "default_vertex_count")]
    vertex_count: u32,
    #[serde(default)]
    capabilities: Vec<Capability>,
    #[serde(default)]
    audio: Vec<AudioCapability>,
    #[serde(default)]
    editor: Vec<EditorCapability>,
    output_bounds: OutputBoundsSchema,
    #[serde(default)]
    properties: Vec<PropertySchema>,
}

impl<'de> Deserialize<'de> for ItemSchema {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let definition = ItemSchemaDefinition::deserialize(deserializer)?;
        let property_abi = PropertyLayout::compile(
            "item",
            &definition.id,
            definition
                .properties
                .iter()
                .map(|property| (property.id(), property.ty())),
        )
        .map_err(D::Error::custom)?;
        let schema = Self {
            id: definition.id,
            label: definition.label,
            category: definition.category,
            tags: definition.tags,
            symbol: definition.symbol,
            shader: definition.shader,
            vertex_count: definition.vertex_count,
            capabilities: definition.capabilities,
            audio: definition.audio,
            editor: definition.editor,
            output_bounds: definition.output_bounds,
            properties: definition.properties,
            property_abi,
        };
        schema.validate().map_err(D::Error::custom)?;
        Ok(schema)
    }
}

impl ItemSchema {
    pub fn shader(&self) -> Option<&ShaderSchema> {
        self.shader.as_ref()
    }

    pub const fn vertex_count(&self) -> u32 {
        self.vertex_count
    }

    pub fn capabilities(&self) -> &[Capability] {
        &self.capabilities
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn label(&self) -> &str {
        self.label.resolve()
    }

    pub fn category(&self) -> &str {
        self.category.label()
    }

    pub fn category_id(&self) -> &str {
        self.category.id()
    }

    pub fn tags(&self) -> &[String] {
        &self.tags
    }

    pub fn symbol(&self) -> &str {
        &self.symbol
    }

    pub fn properties(&self) -> &[PropertySchema] {
        &self.properties
    }

    pub fn output_bounds(&self) -> &OutputBoundsSchema {
        &self.output_bounds
    }

    pub fn property_layout(&self) -> &PropertyLayout {
        &self.property_abi
    }

    pub fn file_properties(&self) -> impl Iterator<Item = &PropertySchema> {
        self.properties.iter().filter(|property| property.is_file())
    }

    pub fn audio(&self) -> &[AudioCapability] {
        &self.audio
    }

    pub fn audio_input(&self, id: &str) -> Option<&AudioCapability> {
        self.audio.iter().find(|input| input.id() == id)
    }

    pub fn timeline(&self) -> Option<TimeMappingProperties<'_>> {
        self.editor
            .iter()
            .find_map(EditorCapability::timeline_properties)
    }

    pub fn file_property(&self, id: &str) -> Option<&PropertySchema> {
        self.property(id).filter(|property| property.is_file())
    }

    pub(super) fn validate(&self) -> Result<(), PluginError> {
        self.category.validate("item", &self.id)?;
        validate_catalog_entry("item", &self.id, &self.label, &self.tags)?;
        validate_property_schemas("item", &self.id, &self.properties)?;
        self.output_bounds
            .validate("item", &self.id, &self.properties)?;
        validate_capabilities("item", &self.id, &self.properties, &self.capabilities)?;
        validate_editors("item", &self.id, &self.properties, &self.editor)?;
        if self.symbol.trim().is_empty() {
            return Err(PluginError::invalid_definition(format!(
                "item '{}' symbol must not be empty",
                self.id
            )));
        }
        if self.shader.is_none() && self.audio.is_empty() && self.editor.is_empty() {
            return Err(PluginError::invalid_definition(format!(
                "item '{}' must define a shader, audio role, or editor role",
                self.id
            )));
        }
        if let Some(shader) = &self.shader {
            shader.validate("item", &self.id)?;
            if self.vertex_count == 0 {
                return Err(PluginError::invalid_definition(format!(
                    "item '{}' vertex count must be non-zero",
                    self.id
                )));
            }
        } else if !self.capabilities.is_empty() {
            return Err(PluginError::invalid_definition(format!(
                "item '{}' has capabilities without a shader",
                self.id
            )));
        }
        let mut audio_inputs = HashSet::new();
        for input in &self.audio {
            input.validate(&self.id, &self.properties)?;
            if !audio_inputs.insert(input.id()) {
                return Err(PluginError::invalid_definition(format!(
                    "item '{}' has duplicate audio input '{}'",
                    self.id,
                    input.id()
                )));
            }
        }
        Ok(())
    }

    pub fn media_sources(&self) -> impl Iterator<Item = crate::media::MediaSource<'_>> {
        self.capabilities
            .iter()
            .filter_map(Capability::media_source)
            .chain(self.audio.iter().map(AudioCapability::media_source))
    }

    pub fn property(&self, id: &str) -> Option<&PropertySchema> {
        self.properties.iter().find(|property| property.id == id)
    }

    pub fn editor(&self) -> &[EditorCapability] {
        &self.editor
    }

    pub fn aspect_lock_property(&self) -> Option<&PropertySchema> {
        let (property, _) = self.editor.iter().find_map(EditorCapability::aspect_lock)?;
        self.property(property)
    }

    pub fn aspect_lock_default(&self) -> bool {
        self.editor
            .iter()
            .find_map(EditorCapability::aspect_lock)
            .is_some_and(|(_, default)| default)
    }

    pub fn size_property(&self) -> Option<&PropertySchema> {
        let property = self.editor.iter().find_map(|editor| match editor {
            EditorCapability::Size { property, .. } => Some(property),
            _ => None,
        })?;
        self.property(property)
    }

    pub fn label_property(&self) -> Option<&PropertySchema> {
        let property = self.editor.iter().find_map(|editor| match editor {
            EditorCapability::Label { property } => Some(property),
            _ => None,
        })?;
        self.property(property)
    }
}

const fn default_vertex_count() -> u32 {
    6
}

impl super::PluginCatalogEntry for ItemSchema {
    fn id(&self) -> &str {
        self.id()
    }

    fn label(&self) -> &str {
        self.label()
    }

    fn category(&self) -> &str {
        self.category()
    }

    fn category_id(&self) -> &str {
        self.category_id()
    }

    fn tags(&self) -> &[String] {
        self.tags()
    }
}
