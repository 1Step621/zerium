//! Item schemas and their runtime parameter ABI.

use crate::domain::parameter::ParameterValueType;
use std::collections::HashSet;

use serde::{Deserialize, Deserializer, de::Error as _};

use super::PluginError;
use super::abi::{CompiledParameterAbi, ParameterAbiField, ParameterInterfaceNames};
use super::capability::{
    AudioCapability, FileCapability, ItemCapabilities, MediaType, VisualCapability,
};
use super::shader::ShaderSchema;
use super::validation::validate_catalog_entry;
use crate::domain::parameter::{
    ParameterSchema, ParameterType, ParameterValues, ScalarParameterType,
};
use crate::domain::plugin::validation::validate_parameter_schemas;

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ItemSchema {
    id: String,
    label: String,
    category: String,
    tags: Vec<String>,
    symbol: String,
    capabilities: ItemCapabilities,
    parameters: Vec<ParameterSchema>,
    parameter_abi: CompiledParameterAbi,
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
    parameters: Vec<ParameterSchema>,
}

impl<'de> Deserialize<'de> for ItemSchema {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let definition = ItemSchemaDefinition::deserialize(deserializer)?;
        let parameter_abi = CompiledParameterAbi::compile(
            "item",
            &definition.id,
            definition
                .parameters
                .iter()
                .map(|parameter| ParameterAbiField {
                    id: parameter.id(),
                    ty: parameter.ty(),
                    static_value: None,
                }),
            ParameterInterfaceNames {
                struct_name: "ZeriumParameters",
                load_function: "zerium_load_parameters",
                raw_load_function: "zerium_raw_params_for_instance",
                accessor_prefix: "zerium_parameter",
                takes_instance_index: true,
            },
        )
        .map_err(D::Error::custom)?;
        let schema = Self {
            id: definition.id,
            label: definition.label,
            category: definition.category,
            tags: definition.tags,
            symbol: definition.symbol,
            capabilities: definition.capabilities,
            parameters: definition.parameters,
            parameter_abi,
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

    pub(crate) fn parameters(&self) -> &[ParameterSchema] {
        &self.parameters
    }

    pub(crate) fn files(&self) -> &[FileCapability] {
        self.capabilities.files()
    }

    pub(crate) fn visual(&self) -> Option<&VisualCapability> {
        self.capabilities.visual()
    }

    pub(crate) fn visual_shader(&self) -> Option<&ShaderSchema> {
        self.visual().map(VisualCapability::shader)
    }

    pub(crate) fn vertex_count(&self) -> Option<u32> {
        self.visual().map(VisualCapability::vertex_count)
    }

    pub(crate) fn is_procedural(&self) -> bool {
        self.visual().is_some_and(VisualCapability::is_procedural)
    }

    pub(crate) fn is_media(&self) -> bool {
        self.visual().is_some_and(VisualCapability::is_media)
    }

    pub(crate) fn is_text(&self) -> bool {
        self.visual().is_some_and(VisualCapability::is_text)
    }

    pub(crate) fn uses_texture_pipeline(&self) -> bool {
        self.visual()
            .is_some_and(VisualCapability::uses_texture_pipeline)
    }

    pub(crate) fn texture_inputs(&self) -> impl Iterator<Item = &FileCapability> {
        self.files()
            .iter()
            .filter(|file| matches!(file.media_type(), MediaType::Video | MediaType::Image))
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
        let mut media_symbols = HashSet::from([
            "zerium_media_inputs".to_owned(),
            "zerium_media_sampler".to_owned(),
        ]);
        for file in self.files() {
            file.validate(&self.id)?;
            if !file_ids.insert(file.id()) {
                return Err(PluginError::invalid_definition(format!(
                    "item '{}' has duplicate file input '{}'",
                    self.id,
                    file.id()
                )));
            }
            for symbol in file.generated_wgsl_symbols() {
                if !media_symbols.insert(symbol.clone()) {
                    return Err(PluginError::invalid_definition(format!(
                        "item '{}' file input '{}' conflicts with generated WGSL symbol '{symbol}'",
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
                .parameter(audio.volume_parameter())
                .map(|parameter| &parameter.ty)
                != Some(&ParameterType::Value(ParameterValueType::Scalar(
                    ScalarParameterType::F32,
                )))
            {
                return Err(PluginError::invalid_definition(format!(
                    "audio item '{}' volume parameter '{}' has the wrong type; expected an f32",
                    self.id,
                    audio.volume_parameter()
                )));
            }
        }
        if let Some(visual) = self.visual() {
            visual.shader().validate("item", &self.id)?;
            if visual.vertex_count() == 0 {
                return Err(PluginError::invalid_definition(format!(
                    "item '{}' vertex count must be non-zero",
                    self.id
                )));
            }
            if visual.is_media()
                && !self
                    .files()
                    .iter()
                    .any(|file| matches!(file.media_type(), MediaType::Video | MediaType::Image))
            {
                return Err(PluginError::invalid_definition(format!(
                    "texture item '{}' must define the file capability",
                    self.id
                )));
            }
        }
        for file in self.files() {
            match file.media_type() {
                MediaType::Video if !self.visual().is_some_and(VisualCapability::is_media) => {
                    return Err(PluginError::invalid_definition(format!(
                        "video item '{}' must define a texture visual capability",
                        self.id
                    )));
                }
                MediaType::Image if !self.visual().is_some_and(VisualCapability::is_media) => {
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

        validate_parameter_schemas("item", &self.id, &self.parameters)?;
        if let Some(editor) = self.capabilities.editor() {
            editor.validate(self)?;
        }
        if let Some(visual) = self.visual() {
            visual.validate_text_parameters(self)?;
        }
        Ok(())
    }

    pub(crate) fn parameter(&self, id: &str) -> Option<&ParameterSchema> {
        self.parameters.iter().find(|parameter| parameter.id == id)
    }

    pub(crate) fn size_parameter(&self) -> Option<&ParameterSchema> {
        self.capabilities
            .editor()?
            .size_parameter()
            .and_then(|id| self.parameter(id))
    }

    pub(crate) fn label_parameter(&self) -> Option<&ParameterSchema> {
        self.capabilities
            .editor()?
            .label_parameter()
            .and_then(|id| self.parameter(id))
    }

    pub(crate) fn supports_aspect_ratio_lock(&self) -> bool {
        self.size_parameter().is_some()
    }

    pub(crate) fn is_size_parameter(&self, parameter_id: &str) -> bool {
        self.capabilities
            .editor()
            .and_then(|editor| editor.size_parameter())
            == Some(parameter_id)
    }

    pub(crate) fn default_parameter_values(&self) -> ParameterValues {
        ParameterValues::for_owner("item", &self.id, &self.parameters)
    }

    /// Returns the typed WGSL API compiled once with this schema.
    pub(crate) fn wgsl_parameter_interface(&self) -> Result<String, PluginError> {
        Ok(self.parameter_abi.interface().to_owned())
    }

    pub(crate) fn pack_parameter_values(
        &self,
        values: &ParameterValues,
    ) -> Result<Vec<u8>, PluginError> {
        values.validate_for("item", &self.id, &self.parameters)?;
        self.parameter_abi.pack("item", &self.id, |id, _| {
            values.get(id).ok_or_else(|| {
                PluginError::invalid_definition(format!(
                    "item '{}' is missing parameter '{id}'",
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
