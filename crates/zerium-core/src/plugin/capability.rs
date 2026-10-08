//! Named shader inputs and editor roles for items and effects.

use crate::property::PropertyValueType;
use std::collections::HashSet;

use serde::Deserialize;

use super::PluginError;
use super::identifier::{validate_logical_id, validate_wgsl_identifier};
use super::validation::PropertyReferences;
use crate::property::{PropertySchema, PropertyType, ScalarPropertyType};

pub(super) const MAX_RENDER_RESULT_OFFSET: u32 = 30;

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
            },
        )?;
    }
    references
        .check(hide_original, "a bool value", |property| {
            property.ty()
                == &PropertyType::Value(PropertyValueType::Scalar(ScalarPropertyType::Bool))
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

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct TextCapability {
    pub id: String,
    pub size: String,
    pub text: String,
    pub font_family: String,
    pub font_size: String,
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
pub enum Capability {
    Media {
        id: String,
        file: String,
        reader: String,
        #[serde(default)]
        placement: Option<MediaPlacement>,
        playback: Option<MediaPlaybackSchema>,
    },
    Text(TextCapability),
    RenderResult {
        id: String,
        start_offset: String,
        end_offset: String,
        hide_original: String,
    },
}

impl Capability {
    pub fn id(&self) -> &str {
        match self {
            Self::Text(text) => &text.id,
            Self::RenderResult { id, .. } => id,
            Self::Media { id, .. } => id,
        }
    }

    pub fn media_source(&self) -> Option<crate::media::MediaSource<'_>> {
        match self {
            Self::Media {
                id, file, reader, ..
            } => Some(crate::media::MediaSource {
                input: crate::media::MediaInputReference::Media(id.clone()),
                file,
                reader,
            }),
            _ => None,
        }
    }

    pub fn playback_properties(&self) -> Option<PlaybackProperties<'_>> {
        match self {
            Self::Media {
                playback: Some(playback),
                ..
            } => Some(playback.playback_properties()),
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
        let context = format!("{owner_kind} '{owner_id}' capability '{}'", self.id());
        let references = PropertyReferences::new(&context, properties);
        match self {
            Self::Text(TextCapability {
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
            }) => {
                references.pair(size)?;
                references.scalar(text, ScalarPropertyType::String)?;
                references.check(font_family, "an array of strings", |property| {
                    matches!(
                        property.ty(),
                        PropertyType::Array {
                            element_type: crate::property::PropertyValueType::Scalar(
                                ScalarPropertyType::String,
                            ),
                            ..
                        }
                    )
                })?;
                references.scalar(font_size, ScalarPropertyType::F32)?;
                references.scalar(color, ScalarPropertyType::Color)?;
                references.scalar(outline_width, ScalarPropertyType::F32)?;
                references.scalar(outline_color, ScalarPropertyType::Color)?;
                references.scalar(bold, ScalarPropertyType::Bool)?;
                references.scalar(italic, ScalarPropertyType::Bool)?;
                for property_id in [horizontal_alignment, vertical_alignment] {
                    references.check(
                        property_id,
                        "an enum containing exactly 0, 1, and 2",
                        |property| matches!(property.ty(), PropertyType::Value(PropertyValueType::Scalar(ScalarPropertyType::Enum(enumeration))) if enumeration.values().len() == 3 && [0, 1, 2].iter().all(|value| enumeration.values().contains(value))),
                    )?;
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
            Self::Media {
                file,
                reader,
                placement,
                playback,
                ..
            } => {
                validate_logical_id("media reader", reader)?;
                references.check(file, "a file property", |property| property.is_file())?;
                if let Some(playback) = playback {
                    playback
                        .playback_properties()
                        .validate(owner_kind, owner_id, properties)?;
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
    source_start: String,
    source_duration: String,
    playback_speed: String,
    end_behavior: String,
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
        }
    }

    pub fn playback_properties(&self) -> PlaybackProperties<'_> {
        PlaybackProperties {
            source_start: &self.source_start,
            source_duration: &self.source_duration,
            playback_speed: &self.playback_speed,
            end_behavior: &self.end_behavior,
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
            property.ty()
                == &PropertyType::Value(PropertyValueType::Scalar(ScalarPropertyType::F32))
        })?;
        self.playback_properties()
            .validate("item", id, properties)?;
        references.check(
            &self.preserve_pitch,
            "a bool value without animation or scene bindings",
            |property| {
                property.ty()
                    == &PropertyType::Value(PropertyValueType::Scalar(ScalarPropertyType::Bool))
                    && !property.is_animatable(None)
                    && !property.is_scene_bindable(None)
            },
        )?;
        Ok(())
    }
}

/// Property references for one visual media input's source clock.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct MediaPlaybackSchema {
    source_start: String,
    source_duration: String,
    playback_speed: String,
    end_behavior: String,
}

impl MediaPlaybackSchema {
    pub fn playback_properties(&self) -> PlaybackProperties<'_> {
        PlaybackProperties {
            source_start: &self.source_start,
            source_duration: &self.source_duration,
            playback_speed: &self.playback_speed,
            end_behavior: &self.end_behavior,
        }
    }
}

/// Ordinary property IDs read independently by each source clock.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PlaybackProperties<'a> {
    pub source_start: &'a str,
    pub source_duration: &'a str,
    pub playback_speed: &'a str,
    pub end_behavior: &'a str,
}

/// Property references for a time mapping, with no input or EOF policy.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TimeMappingProperties<'a> {
    pub source_start: &'a str,
    pub source_duration: &'a str,
    pub playback_speed: &'a str,
}

impl TimeMappingProperties<'_> {
    pub(super) fn validate(
        self,
        owner: &str,
        id: &str,
        properties: &[PropertySchema],
    ) -> Result<(), PluginError> {
        let mut ids = HashSet::new();
        let mut defaults = [0.; 3];
        let invalid_defaults = || {
            PluginError::invalid_definition(format!(
                "{owner} '{id}' has invalid default time mapping values"
            ))
        };
        for ((role, property_id), default) in [
            ("source_start", self.source_start),
            ("source_duration", self.source_duration),
            ("playback_speed", self.playback_speed),
        ]
        .into_iter()
        .zip(&mut defaults)
        {
            let context = format!("{owner} '{id}' time mapping role '{role}'");
            let property = PropertyReferences::new(&context, properties).check(
                property_id,
                "a distinct f32 property without animation or scene bindings",
                |property| {
                    ids.insert(property_id)
                        && property.ty()
                            == &PropertyType::Value(PropertyValueType::Scalar(
                                ScalarPropertyType::F32,
                            ))
                        && !property.is_animatable(None)
                        && !property.is_scene_bindable(None)
                },
            )?;
            let crate::property::PropertyValue::F32(value) = property.default_value() else {
                return Err(invalid_defaults());
            };
            *default = *value;
        }
        crate::timeline::TimeMapping::new(defaults[0], defaults[1], defaults[2])
            .map_err(|_| invalid_defaults())?;
        Ok(())
    }
}

impl<'a> PlaybackProperties<'a> {
    pub fn time_mapping(self) -> TimeMappingProperties<'a> {
        TimeMappingProperties {
            source_start: self.source_start,
            source_duration: self.source_duration,
            playback_speed: self.playback_speed,
        }
    }

    pub(super) fn validate(
        self,
        owner: &str,
        id: &str,
        properties: &[PropertySchema],
    ) -> Result<(), PluginError> {
        self.time_mapping().validate(owner, id, properties)?;
        PropertyReferences::new(&format!("{owner} '{id}' playback"), properties).check(
            self.end_behavior,
            "an enum [0, 1, 2] property without animation or scene bindings",
            |property| {
                matches!(property.ty(), PropertyType::Value(PropertyValueType::Scalar(ScalarPropertyType::Enum(enumeration))) if enumeration.values() == [0, 1, 2])
                    && !property.is_animatable(None)
                    && !property.is_scene_bindable(None)
            },
        ).map(|_| ())
    }
}

pub const MAX_CAPABILITIES: usize = 8;

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
