//! Named shader inputs shared by items and effects, plus item-only roles.

use crate::domain::property::PropertyValueType;
use std::collections::HashSet;

use serde::Deserialize;

use super::PluginError;
use super::identifier::{validate_logical_id, validate_wgsl_identifier};
use super::item::ItemSchema;
use super::shader::ShaderSchema;
use crate::domain::property::{PropertySchema, PropertyType, ScalarPropertyType};

pub(super) const MAX_RENDER_RESULT_OFFSET: u32 = 30;

pub(super) fn validate_render_result_properties(
    owner_kind: &str,
    owner_id: &str,
    properties: &[PropertySchema],
    start_offset: &str,
    end_offset: &str,
    hide_original: &str,
) -> Result<(), PluginError> {
    let property = |id: &str| properties.iter().find(|property| property.id() == id);
    for id in [start_offset, end_offset] {
        let valid = property(id).is_some_and(|property| {
            property.ty()
                == &PropertyType::Value(PropertyValueType::Scalar(ScalarPropertyType::U32))
                && property
                    .configuration_constraints(None)
                    .min
                    .is_some_and(|min| min >= 1.)
                && property
                    .configuration_constraints(None)
                    .max
                    .is_some_and(|max| max <= f64::from(MAX_RENDER_RESULT_OFFSET))
        });
        if !valid {
            return Err(PluginError::invalid_definition(format!(
                "{owner_kind} '{owner_id}' render_result property '{id}' must be u32 constrained to 1..={MAX_RENDER_RESULT_OFFSET}",
            )));
        }
    }
    if property(hide_original).map(PropertySchema::ty)
        != Some(&PropertyType::Value(PropertyValueType::Scalar(
            ScalarPropertyType::Bool,
        )))
    {
        return Err(PluginError::invalid_definition(format!(
            "{owner_kind} '{owner_id}' render_result hide_original property '{hide_original}' must be bool",
        )));
    }
    Ok(())
}

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

    pub(super) fn validate(&self, owner_kind: &str, owner_id: &str) -> Result<(), PluginError> {
        validate_wgsl_identifier("file input", &self.id)?;
        if self.label.trim().is_empty() {
            return Err(PluginError::invalid_definition(format!(
                "{owner_kind} '{owner_id}' file input '{}' has an empty label",
                self.id
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
                    "{owner_kind} '{owner_id}' has invalid file extension '{}'",
                    extension
                )));
            }
            if !extensions.insert(extension) {
                return Err(PluginError::invalid_definition(format!(
                    "{owner_kind} '{owner_id}' has duplicate file extension '{}'",
                    extension
                )));
            }
        }
        Ok(())
    }
}

/// One named texture input produced for an item or effect shader.
#[allow(clippy::large_enum_variant)]
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum Capability {
    Shader {
        id: String,
        shader: ShaderSchema,
        #[serde(default = "default_item_vertex_count")]
        vertex_count: u32,
    },
    Media {
        #[serde(flatten)]
        file: FileCapability,
    },
    Text {
        id: String,
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
    RenderResult {
        id: String,
        start_offset: String,
        end_offset: String,
        hide_original: String,
    },
}

impl Capability {
    pub(crate) fn id(&self) -> &str {
        match self {
            Self::Shader { id, .. } | Self::Text { id, .. } | Self::RenderResult { id, .. } => id,
            Self::Media { file } => file.id(),
        }
    }

    pub(crate) fn shader(&self) -> Option<&ShaderSchema> {
        match self {
            Self::Shader { shader, .. } => Some(shader),
            _ => None,
        }
    }

    pub(crate) fn vertex_count(&self) -> Option<u32> {
        match self {
            Self::Shader { vertex_count, .. } => Some(*vertex_count),
            _ => None,
        }
    }

    pub(crate) fn media_file(&self) -> Option<&FileCapability> {
        match self {
            Self::Media { file } => Some(file),
            _ => None,
        }
    }

    pub(super) fn validate(
        &self,
        owner_kind: &str,
        owner_id: &str,
        properties: &[PropertySchema],
    ) -> Result<(), PluginError> {
        validate_wgsl_identifier("capability", self.id())?;
        if self.id() == "capability_sampler" {
            return Err(PluginError::invalid_definition(format!(
                "{owner_kind} '{owner_id}' capability ID 'capability_sampler' is reserved"
            )));
        }
        match self {
            Self::Shader {
                shader,
                vertex_count,
                ..
            } => {
                shader.validate(owner_kind, owner_id)?;
                if *vertex_count == 0 {
                    return Err(PluginError::invalid_definition(format!(
                        "{owner_kind} '{owner_id}' capability '{}' vertex count must be non-zero",
                        self.id()
                    )));
                }
            }
            Self::Media { file } => {
                if file.media_type() == MediaType::Audio {
                    return Err(PluginError::invalid_definition(format!(
                        "{owner_kind} '{owner_id}' media capability '{}' cannot be audio",
                        self.id()
                    )));
                }
                file.validate(owner_kind, owner_id)?;
            }
            _ => {}
        }
        let mistyped = |property_id: &str, expected: &str| {
            PluginError::invalid_definition(format!(
                "{owner_kind} '{owner_id}' capability property '{}' has the wrong type; expected {expected}",
                property_id
            ))
        };
        let property = |property_id: &str| {
            properties
                .iter()
                .find(|property| property.id() == property_id)
                .ok_or_else(|| {
                    PluginError::invalid_definition(format!(
                        "{owner_kind} '{owner_id}' capability references missing property '{}'",
                        property_id
                    ))
                })
        };
        let tuple_f32_pair = |property_id: &str| {
            let property = property(property_id)?;
            match &property.ty {
                PropertyType::Value(PropertyValueType::Tuple(tuple))
                    if tuple.scalars().len() == 2
                        && tuple.scalars().iter().all(|scalar_type| {
                            *scalar_type == crate::domain::property::ScalarPropertyType::F32
                        }) =>
                {
                    Ok(())
                }
                _ => Err(mistyped(property_id, "a tuple of two f32 values")),
            }
        };
        let scalar = |property_id: &str, ty: ScalarPropertyType| {
            let property = property(property_id)?;
            if property.ty != PropertyType::Value(PropertyValueType::Scalar(ty.clone())) {
                return Err(mistyped(property_id, &format!("a {ty:?} type")));
            }
            Ok(())
        };
        match self {
            Self::Text {
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
            } => {
                tuple_f32_pair(size)?;
                scalar(text, ScalarPropertyType::String)?;
                let font_property = property(font_family)?;
                if !matches!(
                    font_property.ty(),
                    PropertyType::Array {
                        element_type: crate::domain::property::PropertyValueType::Scalar(
                            ScalarPropertyType::String,
                        ),
                        ..
                    }
                ) {
                    return Err(mistyped(font_family, "an array of strings"));
                }
                scalar(font_size, ScalarPropertyType::F32)?;
                scalar(color, ScalarPropertyType::Color)?;
                scalar(outline_width, ScalarPropertyType::F32)?;
                scalar(outline_color, ScalarPropertyType::Color)?;
                scalar(bold, ScalarPropertyType::Bool)?;
                scalar(italic, ScalarPropertyType::Bool)?;
                for property_id in [horizontal_alignment, vertical_alignment] {
                    let property = property(property_id)?;
                    let scalar_type = match property.ty() {
                        PropertyType::Value(value_type) => value_type.scalar_at(None),
                        PropertyType::Array { .. } => None,
                    };
                    let Some(ScalarPropertyType::Enum(enumeration)) = scalar_type else {
                        return Err(mistyped(property_id, "an enum type"));
                    };
                    let values = enumeration.values();
                    if values.len() != 3
                        || !(values.contains(&0) && values.contains(&1) && values.contains(&2))
                    {
                        return Err(mistyped(
                            property_id,
                            "an enum containing exactly 0, 1, and 2",
                        ));
                    }
                }
            }
            Self::RenderResult {
                start_offset,
                end_offset,
                hide_original,
                ..
            } => {
                validate_render_result_properties(
                    owner_kind,
                    owner_id,
                    properties,
                    start_offset,
                    end_offset,
                    hide_original,
                )?;
            }
            Self::Shader { .. } | Self::Media { .. } => {}
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct AudioCapability {
    inputs: Vec<String>,
    #[serde(default)]
    files: Vec<FileCapability>,
    /// Item property read by the host mixer as linear audio gain,
    /// referenced by ID like [`TemporalSamplingSchema`](super::TemporalSamplingSchema) references its
    /// sampling properties.
    volume: String,
}

impl AudioCapability {
    pub(crate) fn inputs(&self) -> &[String] {
        &self.inputs
    }

    pub(crate) fn files(&self) -> &[FileCapability] {
        &self.files
    }

    pub(crate) fn volume_property(&self) -> &str {
        &self.volume
    }

    pub(crate) fn consumes(&self, input_id: &str) -> bool {
        self.inputs.iter().any(|id| id == input_id)
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(super) struct EditorCapability {
    pub(super) position: Option<String>,
    pub(super) size: Option<String>,
    pub(super) points: Option<String>,
    pub(super) label: Option<String>,
}

impl EditorCapability {
    fn is_f32_pair(ty: &PropertyValueType) -> bool {
        matches!(
            ty,
            PropertyValueType::Tuple(tuple)
                if tuple.scalars() == [ScalarPropertyType::F32, ScalarPropertyType::F32]
        )
    }

    pub(super) fn validate(&self, item: &ItemSchema) -> Result<(), PluginError> {
        if self.position.is_none()
            && self.size.is_none()
            && self.points.is_none()
            && self.label.is_none()
        {
            return Err(PluginError::invalid_definition(format!(
                "item '{}' editor capability must reference at least one property",
                item.id()
            )));
        }
        for (kind, property_id) in [
            ("position", self.position.as_deref()),
            ("size", self.size.as_deref()),
        ] {
            let Some(property_id) = property_id else {
                continue;
            };
            if !item.property(property_id).is_some_and(|property| {
                matches!(property.ty(), PropertyType::Value(ty) if Self::is_f32_pair(ty))
            }) {
                return Err(PluginError::invalid_definition(format!(
                    "item '{}' editor {kind} property '{}' must be a tuple of two f32 values",
                    item.id(),
                    property_id
                )));
            }
        }
        if let Some(property_id) = self.points.as_deref() {
            let valid = item.property(property_id).is_some_and(|property| {
                matches!(
                    property.ty(),
                    PropertyType::Array {
                        element_type,
                        ..
                    }
                        if Self::is_f32_pair(element_type)
                )
            });
            if !valid {
                return Err(PluginError::invalid_definition(format!(
                    "item '{}' editor points property '{}' must be an array of two-f32 tuples",
                    item.id(),
                    property_id
                )));
            }
            if self.position.is_none() || self.size.is_none() {
                return Err(PluginError::invalid_definition(format!(
                    "item '{}' editor points property requires position and size properties",
                    item.id()
                )));
            }
        }
        if let Some(property_id) = self.label.as_deref()
            && item.property(property_id).map(|property| property.ty())
                != Some(&PropertyType::Value(PropertyValueType::Scalar(
                    ScalarPropertyType::String,
                )))
        {
            return Err(PluginError::invalid_definition(format!(
                "item '{}' editor label property '{}' must be a string",
                item.id(),
                property_id
            )));
        }
        Ok(())
    }
}

pub(crate) const MAX_CAPABILITIES: usize = 8;

pub(super) fn validate_capabilities(
    owner_kind: &str,
    owner_id: &str,
    properties: &[PropertySchema],
    capabilities: &[Capability],
) -> Result<(), PluginError> {
    if capabilities.len() > MAX_CAPABILITIES {
        return Err(PluginError::invalid_definition(format!(
            "{owner_kind} '{owner_id}' exceeds {MAX_CAPABILITIES} shader capabilities"
        )));
    }
    let mut ids = HashSet::new();
    for capability in capabilities {
        capability.validate(owner_kind, owner_id, properties)?;
        if !ids.insert(capability.id()) {
            return Err(PluginError::invalid_definition(format!(
                "{owner_kind} '{owner_id}' has duplicate capability ID '{}'",
                capability.id()
            )));
        }
    }
    Ok(())
}

const fn default_item_vertex_count() -> u32 {
    6
}

#[cfg(test)]
mod tests {
    use super::Capability;

    #[test]
    fn media_capability_preserves_flat_manifest_contract() {
        let source = r#"{
            "type": "media",
            "id": "source",
            "label": "Source",
            "media_type": "video",
            "reader": "ffmpeg",
            "extensions": ["mp4"]
        }"#;
        let capability: Capability = serde_json::from_str(source).unwrap();
        let file = capability.media_file().unwrap();
        assert_eq!(file.id(), "source");
        assert_eq!(file.reader(), "ffmpeg");
        assert_eq!(file.extensions(), ["mp4"]);

        let unknown_field = source.replace("\"reader\"", "\"unexpected\": true, \"reader\"");
        assert!(serde_json::from_str::<Capability>(&unknown_field).is_err());
    }
}
