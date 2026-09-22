//! Host capabilities declared by item schemas.

use crate::domain::property::PropertyValueType;
use std::collections::HashSet;

use serde::Deserialize;

use super::PluginError;
use super::identifier::{validate_logical_id, validate_media_binding_suffix};
use super::item::ItemSchema;
use super::shader::ShaderSchema;
use crate::domain::property::{PropertyType, ScalarPropertyType};

const MAX_RENDER_RESULT_OFFSET: u32 = 30;

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

    pub(super) fn media_binding_symbols(&self) -> [String; 2] {
        [
            format!("slot_{}", self.id),
            format!("slot_{}_size", self.id),
        ]
    }

    pub(super) fn validate(&self, item_id: &str) -> Result<(), PluginError> {
        validate_media_binding_suffix("file input", &self.id)?;
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

/// Text deliberately carries its property references inline so manifests
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
    /// Host rasterizer inputs, each naming an item property by ID like
    /// [`TemporalSamplingSchema`](super::TemporalSamplingSchema) references its sampling properties.
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
    /// Composites an inclusive range measured backward from the item's layer.
    RenderResult {
        shader: ShaderSchema,
        #[serde(default = "default_item_vertex_count")]
        vertex_count: u32,
        start_offset: String,
        end_offset: String,
        hide_original: String,
    },
}

impl VisualCapability {
    pub(super) fn validate(&self, item: &ItemSchema) -> Result<(), PluginError> {
        self.shader().validate("item", item.id())?;
        let vertex_count = match self {
            Self::Procedural { vertex_count, .. }
            | Self::Media { vertex_count, .. }
            | Self::Text { vertex_count, .. }
            | Self::RenderResult { vertex_count, .. } => *vertex_count,
        };
        if vertex_count == 0 {
            return Err(PluginError::invalid_definition(format!(
                "item '{}' vertex count must be non-zero",
                item.id()
            )));
        }
        if matches!(self, Self::Media { .. }) && item.texture_inputs().next().is_none() {
            return Err(PluginError::invalid_definition(format!(
                "texture item '{}' must define the file capability",
                item.id()
            )));
        }
        let mistyped = |property_id: &str, expected: &str| {
            PluginError::invalid_definition(format!(
                "item '{}' visual property '{}' has the wrong type; expected {expected}",
                item.id(),
                property_id
            ))
        };
        let property = |property_id: &str| {
            item.property(property_id).ok_or_else(|| {
                PluginError::invalid_definition(format!(
                    "item '{}' visual capability references missing property '{}'",
                    item.id(),
                    property_id
                ))
            })
        };
        let tuple_f32_pair = |property_id: &str| {
            let property = property(property_id)?;
            match &property.ty {
                PropertyType::Value(PropertyValueType::Tuple(tuple))
                    if tuple.scalar_count() == 2
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
        let layer_offset = |property_id: &str| {
            scalar(property_id, ScalarPropertyType::U32)?;
            let constraints = property(property_id)?.configuration_constraints(None);
            if constraints.min.is_none_or(|min| min < 1.)
                || constraints
                    .max
                    .is_none_or(|max| max > f64::from(MAX_RENDER_RESULT_OFFSET))
            {
                return Err(mistyped(
                    property_id,
                    &format!("a u32 constrained to 1..={MAX_RENDER_RESULT_OFFSET}"),
                ));
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
                layer_offset(start_offset)?;
                layer_offset(end_offset)?;
                scalar(hide_original, ScalarPropertyType::Bool)?;
            }
            Self::Procedural { .. } | Self::Media { .. } => {}
        }
        Ok(())
    }

    pub(crate) fn shader(&self) -> &ShaderSchema {
        match self {
            Self::Procedural { shader, .. }
            | Self::Media { shader, .. }
            | Self::Text { shader, .. }
            | Self::RenderResult { shader, .. } => shader,
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct AudioCapability {
    inputs: Vec<String>,
    /// Item property read by the host mixer as linear audio gain,
    /// referenced by ID like [`TemporalSamplingSchema`](super::TemporalSamplingSchema) references its
    /// sampling properties.
    volume: String,
}

impl AudioCapability {
    pub(crate) fn inputs(&self) -> &[String] {
        &self.inputs
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
    position: Option<String>,
    size: Option<String>,
    points: Option<String>,
    label: Option<String>,
}

impl EditorCapability {
    fn is_f32_pair(ty: &PropertyValueType) -> bool {
        matches!(
            ty,
            PropertyValueType::Tuple(tuple)
                if tuple.scalars() == [ScalarPropertyType::F32, ScalarPropertyType::F32]
        )
    }

    pub(super) fn position_property(&self) -> Option<&str> {
        self.position.as_deref()
    }

    pub(super) fn size_property(&self) -> Option<&str> {
        self.size.as_deref()
    }

    pub(super) fn points_property(&self) -> Option<&str> {
        self.points.as_deref()
    }

    pub(super) fn label_property(&self) -> Option<&str> {
        self.label.as_deref()
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
            ("position", self.position_property()),
            ("size", self.size_property()),
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
        if let Some(property_id) = self.points_property() {
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
            if self.position_property().is_none() || self.size_property().is_none() {
                return Err(PluginError::invalid_definition(format!(
                    "item '{}' editor points property requires position and size properties",
                    item.id()
                )));
            }
        }
        if let Some(property_id) = self.label_property()
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
