//! Item schemas and their runtime property ABI.

use crate::domain::property::PropertyValueType;
use std::collections::HashSet;

use serde::{Deserialize, Deserializer, de::Error as _};

use super::PluginError;
use super::abi::PropertyLayout;
use super::capability::{
    AudioCapability, FileCapability, ItemCapabilities, MediaType, VisualCapability,
};
use super::validation::{validate_catalog_entry, validate_property_schemas};
use crate::domain::property::{PropertySchema, PropertyType, PropertyValues, ScalarPropertyType};

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ItemSchema {
    id: String,
    label: String,
    category: String,
    tags: Vec<String>,
    symbol: String,
    capabilities: ItemCapabilities,
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
    capabilities: ItemCapabilities,
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
            capabilities: definition.capabilities,
            properties: definition.properties,
            property_abi,
        };
        schema.validate().map_err(D::Error::custom)?;
        Ok(schema)
    }
}

impl ItemSchema {
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

    pub(crate) fn files(&self) -> &[FileCapability] {
        self.capabilities.files()
    }

    pub(crate) fn visual(&self) -> Option<&VisualCapability> {
        self.capabilities.visual()
    }

    pub(crate) fn texture_inputs(&self) -> impl Iterator<Item = &FileCapability> {
        self.files()
            .iter()
            .filter(|file| matches!(file.media_type(), MediaType::Video | MediaType::Image))
    }

    pub(crate) fn texture_input_ids(&self) -> Vec<String> {
        match self.visual() {
            Some(VisualCapability::Text { .. }) => vec!["text".to_owned()],
            Some(VisualCapability::RenderResult { .. }) => vec!["render_result".to_owned()],
            _ => self
                .texture_inputs()
                .map(|input| input.id().to_owned())
                .collect(),
        }
    }

    pub(crate) fn audio(&self) -> Option<&AudioCapability> {
        self.capabilities.audio()
    }

    pub(crate) fn file(&self, id: &str) -> Option<&FileCapability> {
        self.files().iter().find(|file| file.id() == id)
    }

    pub(super) fn validate(&self) -> Result<(), PluginError> {
        validate_catalog_entry("item", &self.id, &self.label, &self.category, &self.tags)?;
        if self.symbol.trim().is_empty() {
            return Err(PluginError::invalid_definition(format!(
                "item '{}' symbol must not be empty",
                self.id
            )));
        }
        if self.capabilities == ItemCapabilities::default() {
            return Err(PluginError::invalid_definition(format!(
                "item '{}' must define at least one capability",
                self.id
            )));
        }
        let mut file_ids = HashSet::new();
        let mut media_symbols =
            HashSet::from(["media_inputs".to_owned(), "media_sampler".to_owned()]);
        for file in self.files() {
            file.validate(&self.id)?;
            if !file_ids.insert(file.id()) {
                return Err(PluginError::invalid_definition(format!(
                    "item '{}' has duplicate file input '{}'",
                    self.id,
                    file.id()
                )));
            }
            for symbol in file.media_binding_symbols() {
                if !media_symbols.insert(symbol.clone()) {
                    return Err(PluginError::invalid_definition(format!(
                        "item '{}' file input '{}' conflicts with media shader symbol '{symbol}'",
                        self.id,
                        file.id()
                    )));
                }
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
                MediaType::Video
                    if !matches!(self.visual(), Some(VisualCapability::Media { .. })) =>
                {
                    return Err(PluginError::invalid_definition(format!(
                        "video item '{}' must define a texture visual capability",
                        self.id
                    )));
                }
                MediaType::Image
                    if !matches!(self.visual(), Some(VisualCapability::Media { .. })) =>
                {
                    return Err(PluginError::invalid_definition(format!(
                        "image item '{}' must define a texture visual capability",
                        self.id
                    )));
                }
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
        if let Some(editor) = self.capabilities.editor() {
            editor.validate(self)?;
        }
        if let Some(visual) = self.visual() {
            visual.validate(self)?;
        }
        Ok(())
    }

    pub(crate) fn property(&self, id: &str) -> Option<&PropertySchema> {
        self.properties.iter().find(|property| property.id == id)
    }

    pub(crate) fn size_property(&self) -> Option<&PropertySchema> {
        self.capabilities
            .editor()?
            .size_property()
            .and_then(|id| self.property(id))
    }

    pub(crate) fn label_property(&self) -> Option<&PropertySchema> {
        self.capabilities
            .editor()?
            .label_property()
            .and_then(|id| self.property(id))
    }

    pub(crate) fn supports_aspect_ratio_lock(&self) -> bool {
        self.size_property().is_some()
    }

    pub(crate) fn is_size_property(&self, property_id: &str) -> bool {
        self.capabilities
            .editor()
            .and_then(|editor| editor.size_property())
            == Some(property_id)
    }

    pub(crate) fn default_property_values(&self) -> PropertyValues {
        PropertyValues::for_owner("item", &self.id, &self.properties)
    }

    pub(crate) fn pack_property_values(
        &self,
        values: &PropertyValues,
    ) -> Result<Vec<u8>, PluginError> {
        values.validate_for("item", &self.id, &self.properties)?;
        self.property_abi.pack("item", &self.id, |id, _| {
            values.property(id).ok_or_else(|| {
                PluginError::invalid_definition(format!(
                    "item '{}' is missing property '{id}'",
                    self.id
                ))
            })
        })
    }
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
