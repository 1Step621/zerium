//! Host capabilities declared by item schemas.

use crate::domain::parameter::ParameterValueType;
use std::collections::HashSet;

use serde::Deserialize;

use super::PluginError;
use super::identifier::{validate_logical_id, validate_wgsl_identifier_suffix};
use super::item::ItemSchema;
use super::shader::ShaderSchema;
use crate::domain::parameter::{ParameterType, ScalarParameterType};

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub(crate) enum MediaType {
    Video,
    Audio,
    Image,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct FileCapability {
    id: String,
    label: String,
    media_type: MediaType,
    reader: String,
    #[serde(default)]
    extensions: Vec<String>,
}

impl FileCapability {
    pub(crate) fn id(&self) -> &str {
        &self.id
    }

    pub(crate) fn label(&self) -> &str {
        &self.label
    }

    pub(crate) const fn media_type(&self) -> MediaType {
        self.media_type
    }

    pub(crate) fn reader(&self) -> &str {
        &self.reader
    }

    pub(crate) fn extensions(&self) -> &[String] {
        &self.extensions
    }

    pub(super) fn generated_wgsl_symbols(&self) -> [String; 2] {
        [
            format!("zerium_media_{}", self.id),
            format!("zerium_media_{}_size", self.id),
        ]
    }

    pub(super) fn validate(&self, item_id: &str) -> Result<(), PluginError> {
        validate_wgsl_identifier_suffix("file input", &self.id)?;
        if matches!(self.id.as_str(), "inputs" | "sampler") {
            return Err(PluginError::invalid_definition(format!(
                "item '{}' file input ID '{}' is reserved by the media WGSL API",
                item_id, self.id
            )));
        }
        if self.label.trim().is_empty() {
            return Err(PluginError::invalid_definition(format!(
                "item '{}' file input '{}' has an empty label",
                item_id, self.id
            )));
        }
        validate_logical_id("media reader", &self.reader)?;
        let mut extensions = HashSet::new();
        for extension in &self.extensions {
            let valid = !extension.is_empty()
                && extension
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit());
            if !valid {
                return Err(PluginError::invalid_definition(format!(
                    "item '{}' has invalid file extension '{}'",
                    item_id, extension
                )));
            }
            if !extensions.insert(extension) {
                return Err(PluginError::invalid_definition(format!(
                    "item '{}' has duplicate file extension '{}'",
                    item_id, extension
                )));
            }
        }
        Ok(())
    }
}

/// Text deliberately carries its parameter references inline so manifests
/// stay flat; capabilities are shared by `Arc`, not moved per frame.
#[allow(clippy::large_enum_variant)]
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum VisualCapability {
    Procedural {
        shader: ShaderSchema,
        #[serde(default = "default_item_vertex_count")]
        vertex_count: u32,
    },
    Media {
        shader: ShaderSchema,
        #[serde(default = "default_item_vertex_count")]
        vertex_count: u32,
    },
    /// Host rasterizer inputs, each naming an item parameter by ID like
    /// [`TemporalSamplingSchema`](super::TemporalSamplingSchema) references its sampling parameters.
    Text {
        shader: ShaderSchema,
        #[serde(default = "default_item_vertex_count")]
        vertex_count: u32,
        size: String,
        text: String,
        font_family: String,
        font_size: String,
        color: String,
        outline_width: String,
        outline_color: String,
        bold: String,
        italic: String,
        horizontal_alignment: String,
        vertical_alignment: String,
    },
}

impl VisualCapability {
    pub(super) fn validate_text_parameters(&self, item: &ItemSchema) -> Result<(), PluginError> {
        let Self::Text {
            size,
            text,
            font_family,
            font_size,
            color,
            outline_width,
            outline_color,
            bold,
            italic,
            horizontal_alignment,
            vertical_alignment,
            ..
        } = self
        else {
            return Ok(());
        };
        let mistyped = |parameter_id: &str, expected: &str| {
            PluginError::invalid_definition(format!(
                "text item '{}' text parameter '{}' has the wrong type; expected {expected}",
                item.id(),
                parameter_id
            ))
        };
        let parameter = |parameter_id: &str| {
            item.parameter(parameter_id).ok_or_else(|| {
                PluginError::invalid_definition(format!(
                    "text item '{}' is missing text parameter '{}'",
                    item.id(),
                    parameter_id
                ))
            })
        };
        let tuple_f32_pair = |parameter_id: &str| {
            let parameter = parameter(parameter_id)?;
            match &parameter.ty {
                ParameterType::Value(ParameterValueType::Tuple(tuple))
                    if tuple.element_count() == 2
                        && tuple.elements().iter().all(|element| {
                            *element == crate::domain::parameter::ScalarParameterType::F32
                        }) =>
                {
                    Ok(())
                }
                _ => Err(mistyped(parameter_id, "a tuple of two f32 values")),
            }
        };
        let scalar = |parameter_id: &str, ty: ScalarParameterType| {
            let parameter = parameter(parameter_id)?;
            if parameter.ty != ParameterType::Value(ParameterValueType::Scalar(ty.clone())) {
                return Err(mistyped(parameter_id, &format!("a {ty:?} type")));
            }
            Ok(())
        };
        tuple_f32_pair(size)?;
        scalar(text, ScalarParameterType::String)?;
        let font_parameter = parameter(font_family)?;
        if !matches!(
            font_parameter.ty().array_element_type(),
            Some(crate::domain::parameter::ParameterValueType::Scalar(
                ScalarParameterType::String
            ))
        ) {
            return Err(mistyped(font_family, "an array of strings"));
        }
        scalar(font_size, ScalarParameterType::F32)?;
        scalar(color, ScalarParameterType::Color)?;
        scalar(outline_width, ScalarParameterType::F32)?;
        scalar(outline_color, ScalarParameterType::Color)?;
        scalar(bold, ScalarParameterType::Bool)?;
        scalar(italic, ScalarParameterType::Bool)?;
        for parameter_id in [horizontal_alignment, vertical_alignment] {
            let parameter = parameter(parameter_id)?;
            let Some(ScalarParameterType::Enum(enumeration)) = parameter.ty().scalar_type() else {
                return Err(mistyped(parameter_id, "an enum type"));
            };
            let values = enumeration.values();
            if values.len() != 3
                || !(values.contains(&0) && values.contains(&1) && values.contains(&2))
            {
                return Err(mistyped(
                    parameter_id,
                    "an enum containing exactly 0, 1, and 2",
                ));
            }
        }
        Ok(())
    }

    pub(crate) fn shader(&self) -> &ShaderSchema {
        match self {
            Self::Procedural { shader, .. }
            | Self::Media { shader, .. }
            | Self::Text { shader, .. } => shader,
        }
    }

    pub(crate) const fn vertex_count(&self) -> u32 {
        match self {
            Self::Procedural { vertex_count, .. }
            | Self::Media { vertex_count, .. }
            | Self::Text { vertex_count, .. } => *vertex_count,
        }
    }

    pub(crate) const fn is_procedural(&self) -> bool {
        matches!(self, Self::Procedural { .. })
    }

    pub(crate) const fn is_media(&self) -> bool {
        matches!(self, Self::Media { .. })
    }

    pub(crate) const fn uses_texture_pipeline(&self) -> bool {
        matches!(self, Self::Media { .. } | Self::Text { .. })
    }

    pub(crate) const fn is_text(&self) -> bool {
        matches!(self, Self::Text { .. })
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct AudioCapability {
    inputs: Vec<String>,
    /// Item parameter read by the host mixer as linear audio gain,
    /// referenced by ID like [`TemporalSamplingSchema`](super::TemporalSamplingSchema) references its
    /// sampling parameters.
    volume: String,
}

impl AudioCapability {
    pub(crate) fn inputs(&self) -> &[String] {
        &self.inputs
    }

    pub(crate) fn volume_parameter(&self) -> &str {
        &self.volume
    }

    pub(crate) fn consumes(&self, input_id: &str) -> bool {
        self.inputs.iter().any(|id| id == input_id)
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(super) struct EditorCapability {
    size: Option<String>,
    label: Option<String>,
}

impl EditorCapability {
    pub(super) fn size_parameter(&self) -> Option<&str> {
        self.size.as_deref()
    }

    pub(super) fn label_parameter(&self) -> Option<&str> {
        self.label.as_deref()
    }

    pub(super) fn validate(&self, item: &ItemSchema) -> Result<(), PluginError> {
        if self.size.is_none() && self.label.is_none() {
            return Err(PluginError::invalid_definition(format!(
                "item '{}' editor capability must reference at least one parameter",
                item.id()
            )));
        }
        if let Some(parameter_id) = self.size_parameter() {
            let valid = item.parameter(parameter_id).is_some_and(|parameter| {
                matches!(
                    parameter.ty(),
                    ParameterType::Value(ParameterValueType::Tuple(tuple))
                        if tuple.element_count() == 2
                            && tuple.elements().iter().all(|element| {
                                *element == crate::domain::parameter::ScalarParameterType::F32
                            })
                )
            });
            if !valid {
                return Err(PluginError::invalid_definition(format!(
                    "item '{}' editor size parameter '{}' must be a tuple of two f32 values",
                    item.id(),
                    parameter_id
                )));
            }
        }
        if let Some(parameter_id) = self.label_parameter()
            && item.parameter(parameter_id).map(|parameter| parameter.ty())
                != Some(&ParameterType::Value(ParameterValueType::Scalar(
                    ScalarParameterType::String,
                )))
        {
            return Err(PluginError::invalid_definition(format!(
                "item '{}' editor label parameter '{}' must be a string",
                item.id(),
                parameter_id
            )));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(super) struct ItemCapabilities {
    #[serde(default)]
    files: Vec<FileCapability>,
    visual: Option<VisualCapability>,
    audio: Option<AudioCapability>,
    editor: Option<EditorCapability>,
}

impl ItemCapabilities {
    pub(super) fn files(&self) -> &[FileCapability] {
        &self.files
    }

    pub(super) fn visual(&self) -> Option<&VisualCapability> {
        self.visual.as_ref()
    }

    pub(super) fn audio(&self) -> Option<&AudioCapability> {
        self.audio.as_ref()
    }

    pub(super) fn editor(&self) -> Option<&EditorCapability> {
        self.editor.as_ref()
    }
}

const fn default_item_vertex_count() -> u32 {
    6
}
