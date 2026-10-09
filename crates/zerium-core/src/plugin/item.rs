//! Item schemas and their runtime property ABI.

use crate::localized_text::LocalizedText;
use std::collections::HashSet;

use serde::{Deserialize, Deserializer, de::Error as _};

use super::ItemCategory;
use super::OutputBoundsSchema;
use super::PluginError;
use super::abi::PropertyLayout;
use super::capability::{
    AudioCapability, TextureInput, TimeMappingProperties, validate_texture_inputs,
};
use super::editor::{EditorCapability, validate_editors};
use super::shader::ShaderSchema;
use super::validation::{validate_catalog_entry, validate_property_schemas};
use crate::property::PropertySchema;

#[derive(Clone, Debug, PartialEq)]
pub struct ItemSchema {
    id: String,
    label: LocalizedText,
    category: ItemCategory,
    tags: Vec<String>,
    symbol: String,
    render: Option<ItemRenderSchema>,
    audio: Vec<AudioCapability>,
    editor: Vec<EditorCapability>,
    property_layout: PropertyLayout,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ItemRenderSchema {
    pub shader: ShaderSchema,
    #[serde(default = "default_vertex_count")]
    pub vertex_count: u32,
    pub bounds: OutputBoundsSchema,
    #[serde(default)]
    pub inputs: Vec<TextureInput>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ItemSchemaDefinition {
    id: String,
    label: LocalizedText,
    category: ItemCategory,
    #[serde(default)]
    tags: Vec<String>,
    symbol: String,
    #[serde(default)]
    render: Option<ItemRenderSchema>,
    #[serde(default)]
    audio: Vec<AudioCapability>,
    #[serde(default)]
    editor: Vec<EditorCapability>,
    #[serde(default)]
    properties: Vec<PropertySchema>,
}

impl<'de> Deserialize<'de> for ItemSchema {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let definition = ItemSchemaDefinition::deserialize(deserializer)?;
        let property_layout =
            PropertyLayout::compile("item", &definition.id, definition.properties)
                .map_err(D::Error::custom)?;
        let schema = Self {
            id: definition.id,
            label: definition.label,
            category: definition.category,
            tags: definition.tags,
            symbol: definition.symbol,
            render: definition.render,
            audio: definition.audio,
            editor: definition.editor,
            property_layout,
        };
        schema.validate().map_err(D::Error::custom)?;
        Ok(schema)
    }
}

impl ItemSchema {
    pub fn render(&self) -> Option<&ItemRenderSchema> {
        self.render.as_ref()
    }

    pub fn inputs(&self) -> &[TextureInput] {
        self.render
            .as_ref()
            .map_or(&[], |render| render.inputs.as_slice())
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn label(&self) -> &str {
        self.label.resolve()
    }

    pub fn category(&self) -> ItemCategory {
        self.category
    }

    pub fn tags(&self) -> &[String] {
        &self.tags
    }

    pub fn symbol(&self) -> &str {
        &self.symbol
    }

    pub fn properties(&self) -> &[PropertySchema] {
        self.property_layout.properties()
    }

    pub fn property_layout(&self) -> &PropertyLayout {
        &self.property_layout
    }

    pub fn file_properties(&self) -> impl Iterator<Item = &PropertySchema> {
        self.properties()
            .iter()
            .filter(|property| property.is_file())
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
        validate_catalog_entry("item", &self.id, &self.label, &self.tags)?;
        validate_property_schemas("item", &self.id, self.properties())?;
        if let Some(render) = &self.render {
            render
                .bounds
                .validate("item", &self.id, self.properties())?;
            validate_texture_inputs("item", &self.id, self.properties(), &render.inputs)?;
            render.shader.validate("item", &self.id)?;
            if render.vertex_count == 0 {
                return Err(PluginError::invalid_definition(format!(
                    "item '{}' vertex count must be non-zero",
                    self.id
                )));
            }
        }
        validate_editors("item", &self.id, self.properties(), &self.editor)?;
        if self.symbol.trim().is_empty() {
            return Err(PluginError::invalid_definition(format!(
                "item '{}' symbol must not be empty",
                self.id
            )));
        }
        if self.render.is_none() && self.audio.is_empty() && self.editor.is_empty() {
            return Err(PluginError::invalid_definition(format!(
                "item '{}' must define render, audio, or editor behavior",
                self.id
            )));
        }
        let mut audio_inputs = HashSet::new();
        for input in &self.audio {
            input.validate(&self.id, self.properties())?;
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
        self.inputs()
            .iter()
            .filter_map(TextureInput::media_source)
            .chain(self.audio.iter().map(AudioCapability::media_source))
    }

    pub fn property(&self, id: &str) -> Option<&PropertySchema> {
        self.properties().iter().find(|property| property.id == id)
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
    type Category = ItemCategory;

    fn id(&self) -> &str {
        self.id()
    }

    fn label(&self) -> &str {
        self.label()
    }

    fn category(&self) -> ItemCategory {
        self.category
    }

    fn tags(&self) -> &[String] {
        self.tags()
    }
}
