//! Item schemas and their runtime property ABI.

use crate::domain::property::PropertyValueType;
use std::collections::HashSet;

use serde::{Deserialize, Deserializer, de::Error as _};

use super::PluginError;
use super::abi::PropertyLayout;
use super::capability::{
    AudioCapability, Capability, EditorCapability, FileCapability, MediaType, validate_capabilities,
};
use super::shader::ShaderSchema;
use super::validation::{validate_catalog_entry, validate_property_schemas};
use crate::domain::property::{PropertySchema, PropertyType, ScalarPropertyType};

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ItemSchema {
    id: String,
    label: String,
    category: String,
    tags: Vec<String>,
    symbol: String,
    shader: Option<ShaderSchema>,
    vertex_count: u32,
    capabilities: Vec<Capability>,
    audio: Option<AudioCapability>,
    editor: Option<EditorCapability>,
    properties: Vec<PropertySchema>,
    property_abi: PropertyLayout,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ItemSchemaDefinition {
    id: String,
    label: String,
    category: String,
    #[serde(default)]
    tags: Vec<String>,
    symbol: String,
    #[serde(default)]
    shader: Option<ShaderSchema>,
    #[serde(default = "default_vertex_count")]
    vertex_count: u32,
    #[serde(default)]
    capabilities: Vec<Capability>,
    audio: Option<AudioCapability>,
    editor: Option<EditorCapability>,
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
            properties: definition.properties,
            property_abi,
        };
        schema.validate().map_err(D::Error::custom)?;
        Ok(schema)
    }
}

impl ItemSchema {
    pub(crate) fn shader(&self) -> Option<&ShaderSchema> {
        self.shader.as_ref()
    }

    pub(crate) const fn vertex_count(&self) -> u32 {
        self.vertex_count
    }

    pub(crate) fn capabilities(&self) -> &[Capability] {
        &self.capabilities
    }

    pub(crate) fn id(&self) -> &str {
        &self.id
    }

    pub(crate) fn label(&self) -> &str {
        &self.label
    }

    pub(crate) fn category(&self) -> &str {
        &self.category
    }

    pub(crate) fn tags(&self) -> &[String] {
        &self.tags
    }

    pub(crate) fn symbol(&self) -> &str {
        &self.symbol
    }

    pub(crate) fn properties(&self) -> &[PropertySchema] {
        &self.properties
    }

    pub(crate) fn property_layout(&self) -> &PropertyLayout {
        &self.property_abi
    }

    pub(crate) fn files(&self) -> impl Iterator<Item = &FileCapability> {
        self.capabilities
            .iter()
            .filter_map(Capability::media_file)
            .chain(self.audio.iter().flat_map(AudioCapability::files))
    }

    pub(crate) fn audio(&self) -> Option<&AudioCapability> {
        self.audio.as_ref()
    }

    pub(crate) fn file(&self, id: &str) -> Option<&FileCapability> {
        self.files().find(|file| file.id() == id)
    }

    pub(super) fn validate(&self) -> Result<(), PluginError> {
        validate_catalog_entry("item", &self.id, &self.label, &self.category, &self.tags)?;
        if self.symbol.trim().is_empty() {
            return Err(PluginError::invalid_definition(format!(
                "item '{}' symbol must not be empty",
                self.id
            )));
        }
        if self.shader.is_none() && self.audio.is_none() && self.editor.is_none() {
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
        validate_capabilities("item", &self.id, &self.properties, &self.capabilities)?;
        let mut file_ids = HashSet::new();
        for file in self.files() {
            file.validate("item", &self.id)?;
            if !file_ids.insert(file.id()) {
                return Err(PluginError::invalid_definition(format!(
                    "item '{}' has duplicate file input '{}'",
                    self.id,
                    file.id()
                )));
            }
        }
        if let Some(audio) = self.audio() {
            if audio.inputs().is_empty() {
                return Err(PluginError::invalid_definition(format!(
                    "audio item '{}' must consume at least one file input",
                    self.id
                )));
            }
            let mut audio_inputs = HashSet::new();
            for input_id in audio.inputs() {
                if !audio_inputs.insert(input_id) {
                    return Err(PluginError::invalid_definition(format!(
                        "audio item '{}' consumes file input '{}' more than once",
                        self.id, input_id
                    )));
                }
                let Some(file) = self.file(input_id) else {
                    return Err(PluginError::invalid_definition(format!(
                        "audio item '{}' consumes unknown file input '{}'",
                        self.id, input_id
                    )));
                };
                if !matches!(file.media_type(), MediaType::Video | MediaType::Audio) {
                    return Err(PluginError::invalid_definition(format!(
                        "audio item '{}' consumes non-audio file input '{}'",
                        self.id, input_id
                    )));
                }
            }
            if self
                .property(audio.volume_property())
                .map(|property| &property.ty)
                != Some(&PropertyType::Value(PropertyValueType::Scalar(
                    ScalarPropertyType::F32,
                )))
            {
                return Err(PluginError::invalid_definition(format!(
                    "audio item '{}' volume property '{}' has the wrong type; expected an f32",
                    self.id,
                    audio.volume_property()
                )));
            }
        }
        for file in self.files() {
            match file.media_type() {
                MediaType::Audio
                    if !self.audio().is_some_and(|audio| audio.consumes(file.id())) =>
                {
                    return Err(PluginError::invalid_definition(format!(
                        "audio item '{}' must define the audio capability",
                        self.id
                    )));
                }
                _ => {}
            }
        }

        validate_property_schemas("item", &self.id, &self.properties)?;
        if let Some(editor) = &self.editor {
            editor.validate(self)?;
        }
        Ok(())
    }

    pub(crate) fn property(&self, id: &str) -> Option<&PropertySchema> {
        self.properties.iter().find(|property| property.id == id)
    }

    pub(crate) fn size_property(&self) -> Option<&PropertySchema> {
        self.editor
            .as_ref()?
            .size
            .as_deref()
            .and_then(|id| self.property(id))
    }

    pub(crate) fn position_property(&self) -> Option<&PropertySchema> {
        self.editor
            .as_ref()?
            .position
            .as_deref()
            .and_then(|id| self.property(id))
    }

    pub(crate) fn points_property(&self) -> Option<&PropertySchema> {
        self.editor
            .as_ref()?
            .points
            .as_deref()
            .and_then(|id| self.property(id))
    }

    pub(crate) fn label_property(&self) -> Option<&PropertySchema> {
        self.editor
            .as_ref()?
            .label
            .as_deref()
            .and_then(|id| self.property(id))
    }

    pub(crate) fn supports_aspect_ratio_lock(&self) -> bool {
        self.size_property().is_some()
    }

    pub(crate) fn is_size_property(&self, property_id: &str) -> bool {
        self.editor
            .as_ref()
            .and_then(|editor| editor.size.as_deref())
            == Some(property_id)
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

    fn tags(&self) -> &[String] {
        self.tags()
    }
}
