//! Named shader inputs and editor roles for items and effects.

use std::collections::HashSet;

use serde::Deserialize;

use super::PluginError;
use super::validation::{
    PropertyReferences, has_enum_values, validate_logical_id, validate_wgsl_identifier,
};
use crate::property::{PropertyOverrides, PropertySchema, PropertyValue, ScalarPropertyType};

pub(super) const MAX_RENDER_RESULT_OFFSET: u32 = 30;

/// Maximum fixed decimal precision of a number input.
pub const MAX_DECIMAL_PLACES: u32 = 10;

pub(super) fn validate_render_result_properties(
    owner_kind: &str,
    owner_id: &str,
    properties: &[PropertySchema],
    start_offset: &str,
    end_offset: &str,
    hide_original: &str,
) -> Result<(), PluginError> {
    let context = format!("{owner_kind} '{owner_id}' render_result");
    let references = PropertyReferences::new(&context, properties);
    for id in [start_offset, end_offset] {
        references.check(
            id,
            &format!("u32 constrained to 1..={MAX_RENDER_RESULT_OFFSET}"),
            |property| {
                property.scalar_type(None, None) == Some(&ScalarPropertyType::U32)
                    && property
                        .configuration_constraints(None)
                        .min
                        .is_some_and(|min| min >= 1.)
                    && property
                        .configuration_constraints(None)
                        .max
                        .is_some_and(|max| max <= f64::from(MAX_RENDER_RESULT_OFFSET))
            },
        )?;
    }

    references
        .check(hide_original, "a bool value", |property| {
            property.scalar_type(None, None) == Some(&ScalarPropertyType::Bool)
        })
        .map(|_| ())
}

/// Describes where a media input appears in its owner's composition space.
/// The quad covers the full source UV range.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct MediaPlacement {
    pub position: String,
    pub size: String,
    pub origin: Option<String>,
}

/// Camera coordinates for a render-result capture, resolved from owner properties.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct RenderResultView {
    pub position: String,
    pub zoom: String,
    pub angle: String,
}

/// Property references shared by text and number rasterization.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct TextStyle {
    pub size: String,
    pub font_family: String,
    pub font_size: String,
    pub letter_spacing: String,
    pub line_spacing: String,
    pub color: String,
    pub outline_width: String,
    pub outline_color: String,
    pub bold: String,
    pub italic: String,
    pub horizontal_alignment: String,
    pub vertical_alignment: String,
}

/// One named texture input produced for an item or effect shader.
#[allow(clippy::large_enum_variant)]
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum TextureInput {
    Media {
        id: String,
        file: String,
        reader: String,
        #[serde(default)]
        placement: Option<MediaPlacement>,
        frame_rate: Option<String>,
    },
    Text {
        id: String,
        text: String,
        style: TextStyle,
    },
    Number {
        id: String,
        value: String,
        decimal_places: String,
        style: TextStyle,
    },
    RenderResult {
        id: String,
        start_offset: String,
        end_offset: String,
        hide_original: String,
        view: Option<RenderResultView>,
    },
}

impl TextureInput {
    pub fn id(&self) -> &str {
        match self {
            Self::Text { id, .. }
            | Self::Number { id, .. }
            | Self::RenderResult { id, .. }
            | Self::Media { id, .. } => id,
        }
    }

    pub fn text_style(&self) -> Option<&TextStyle> {
        match self {
            Self::Text { style, .. } | Self::Number { style, .. } => Some(style),
            _ => None,
        }
    }

    pub fn media_source(&self) -> Option<crate::media::MediaSource<'_>> {
        match self {
            Self::Media {
                id,
                file,
                reader,
                frame_rate,
                ..
            } => Some(crate::media::MediaSource {
                input: crate::media::MediaInputReference::Media(id.clone()),
                file,
                reader,
                frame_rate: frame_rate.as_deref(),
            }),
            _ => None,
        }
    }

    pub(super) fn validate(
        &self,
        owner_kind: &str,
        owner_id: &str,
        properties: &[PropertySchema],
    ) -> Result<(), PluginError> {
        validate_wgsl_identifier("texture input", self.id())?;
        let context = format!("{owner_kind} '{owner_id}' input '{}'", self.id());
        let references = PropertyReferences::new(&context, properties);
        match self {
            Self::Text { text, .. } => {
                references.scalar(text, ScalarPropertyType::String)?;
            }
            Self::Number {
                value,
                decimal_places,
                ..
            } => {
                references.check(value, "an f32, i32, or u32 value", |property| {
                    matches!(
                        property.scalar_type(None, None),
                        Some(
                            ScalarPropertyType::F32
                                | ScalarPropertyType::I32
                                | ScalarPropertyType::U32
                        )
                    )
                })?;
                references.check(
                    decimal_places,
                    &format!("a u32 constrained to 0..={MAX_DECIMAL_PLACES}"),
                    |property| {
                        property.scalar_type(None, None) == Some(&ScalarPropertyType::U32)
                            && property
                                .configuration_constraints(None)
                                .max
                                .is_some_and(|max| max <= f64::from(MAX_DECIMAL_PLACES))
                    },
                )?;
            }

            Self::RenderResult {
                start_offset,
                end_offset,
                hide_original,
                view,
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

                if let Some(view) = view {
                    references.pair(&view.position)?;
                    references.scalar(&view.angle, ScalarPropertyType::F32)?;
                    references.check(&view.zoom, "an f32 with a positive minimum", |property| {
                        property.scalar_type(None, None) == Some(&ScalarPropertyType::F32)
                            && property
                                .configuration_constraints(None)
                                .min
                                .is_some_and(|minimum| minimum > 0.)
                    })?;
                }
            }

            Self::Media {
                file,
                reader,
                placement,
                frame_rate,
                ..
            } => {
                validate_logical_id("media reader", reader)?;
                references.check(file, "a file property", |property| property.is_file())?;
                if let Some(frame_rate) = frame_rate {
                    references.check(
                        frame_rate,
                        "an f32 constrained to 0.001..=1000 without animation or scene bindings",
                        |property| {
                            let constraints = property.configuration_constraints(None);
                            property.scalar_type(None, None) == Some(&ScalarPropertyType::F32)
                                && !property.is_animatable(None)
                                && !property.is_scene_bindable(None)
                                && constraints.min.is_some_and(|min| min >= 0.001)
                                && constraints.max.is_some_and(|max| max <= 1000.)
                        },
                    )?;
                }

                if let Some(MediaPlacement {
                    position,
                    size,
                    origin,
                }) = placement
                {
                    references.pair(position)?;
                    references.pair(size)?;

                    if let Some(origin) = origin {
                        references.origin(origin)?;
                    }
                }
            }
        }

        if let Some(style) = self.text_style() {
            style.validate(&references)?;
        }
        Ok(())
    }
}

impl TextStyle {
    fn validate(&self, references: &PropertyReferences<'_>) -> Result<(), PluginError> {
        references.pair(&self.size)?;
        references.check(&self.font_family, "an array of strings", |property| {
            property
                .array()
                .and_then(|array| array.element.scalar(None))
                .is_some_and(|scalar| scalar.ty == ScalarPropertyType::String)
        })?;
        references.scalar(&self.font_size, ScalarPropertyType::F32)?;
        references.scalar(&self.letter_spacing, ScalarPropertyType::F32)?;
        references.scalar(&self.line_spacing, ScalarPropertyType::F32)?;
        references.scalar(&self.color, ScalarPropertyType::Color)?;
        references.scalar(&self.outline_width, ScalarPropertyType::F32)?;
        references.scalar(&self.outline_color, ScalarPropertyType::Color)?;
        references.scalar(&self.bold, ScalarPropertyType::Bool)?;
        references.scalar(&self.italic, ScalarPropertyType::Bool)?;

        for property_id in [&self.horizontal_alignment, &self.vertical_alignment] {
            references.check(
                property_id,
                "an enum containing exactly 0, 1, and 2",
                |property| {
                    property
                        .scalar_type(None, None)
                        .is_some_and(|ty| has_enum_values(ty, &[0, 1, 2]))
                },
            )?;
        }
        Ok(())
    }
}

/// One named audio stream and its independently referenced playback settings.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct AudioCapability {
    id: String,
    file: String,
    reader: String,
    volume: String,
    preserve_pitch: String,
}

impl AudioCapability {
    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn media_source(&self) -> crate::media::MediaSource<'_> {
        crate::media::MediaSource {
            input: crate::media::MediaInputReference::Audio(self.id.clone()),
            file: &self.file,
            reader: &self.reader,
            frame_rate: None,
        }
    }

    pub fn preserve_pitch_property(&self) -> &str {
        &self.preserve_pitch
    }

    pub fn volume_property(&self) -> &str {
        &self.volume
    }

    pub(super) fn validate(
        &self,
        id: &str,
        properties: &[PropertySchema],
    ) -> Result<(), PluginError> {
        validate_logical_id("audio input", &self.id)?;
        validate_logical_id("audio reader", &self.reader)?;
        let context = format!("item '{id}' audio input '{}'", self.id());
        let references = PropertyReferences::new(&context, properties);
        references.check(&self.file, "a file property", |property| property.is_file())?;
        references.check(&self.volume, "an f32 value", |property| {
            property.scalar_type(None, None) == Some(&ScalarPropertyType::F32)
        })?;
        references.check(
            &self.preserve_pitch,
            "a bool value without animation or scene bindings",
            |property| {
                property.scalar_type(None, None) == Some(&ScalarPropertyType::Bool)
                    && !property.is_animatable(None)
                    && !property.is_scene_bindable(None)
            },
        )?;
        Ok(())
    }
}

pub const MAX_TEXTURE_INPUTS: usize = 8;

pub(super) fn validate_texture_inputs(
    owner_kind: &str,
    owner_id: &str,
    properties: &[PropertySchema],
    inputs: &[TextureInput],
) -> Result<(), PluginError> {
    if inputs.len() > MAX_TEXTURE_INPUTS {
        return Err(PluginError::invalid_definition(format!(
            "{owner_kind} '{owner_id}' exceeds {MAX_TEXTURE_INPUTS} texture inputs"
        )));
    }

    let mut ids = HashSet::new();
    for input in inputs {
        input.validate(owner_kind, owner_id, properties)?;

        if !ids.insert(input.id()) {
            return Err(PluginError::invalid_definition(format!(
                "{owner_kind} '{owner_id}' has duplicate input ID '{}'",
                input.id()
            )));
        }
    }

    Ok(())
}

/// Sources without embedded timing inherit project FPS only when created.
pub(crate) fn initialize_frame_rates(
    inputs: &[TextureInput],
    schemas: &[PropertySchema],
    values: &mut PropertyOverrides,
    rate: crate::timeline::FrameRate,
) {
    let rate = PropertyValue::F32(rate.frames_per_second() as f32);
    for input in inputs {
        if let TextureInput::Media {
            frame_rate: Some(id),
            ..
        } = input
        {
            let property = schemas
                .iter()
                .find(|property| property.id() == id)
                .expect("validated frame-rate property");
            if property.accepts_value(&rate) {
                values
                    .set(property, rate.clone())
                    .expect("valid source FPS");
            }
        }
    }
}
