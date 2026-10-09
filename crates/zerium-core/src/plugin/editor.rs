//! Independent editor tools and their complete property references.
use std::collections::HashSet;

use serde::Deserialize;

use super::validation::{PropertyReferences, is_f32_pair};
use super::{PluginError, TimeMappingProperties};
use crate::property::{PropertyDefinition, PropertySchema, ScalarPropertyType};

/// One editor feature; each declaration owns all references it consumes.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum EditorCapability {
    Timeline {
        source_start: String,
        source_duration: String,
        playback_speed: String,
    },
    Position {
        property: String,
    },
    Size {
        property: String,
        position: String,
        origin: Option<String>,
    },
    AspectLock {
        property: String,
        #[serde(default)]
        default: bool,
    },
    Points {
        property: String,
        position: String,
        size: String,
        origin: Option<String>,
    },
    Spline {
        points: String,
        position: String,
        size: String,
        origin: Option<String>,
        tension: String,
        closed: String,
    },
    Label {
        property: String,
    },
}

impl EditorCapability {
    fn kind(&self) -> &'static str {
        match self {
            Self::Timeline { .. } => "timeline",
            Self::Position { .. } => "position",
            Self::Size { .. } => "size",
            Self::AspectLock { .. } => "aspect_lock",
            Self::Points { .. } => "points",
            Self::Spline { .. } => "spline",
            Self::Label { .. } => "label",
        }
    }

    pub(super) fn timeline_properties(&self) -> Option<TimeMappingProperties<'_>> {
        match self {
            Self::Timeline {
                source_start,
                source_duration,
                playback_speed,
            } => Some(TimeMappingProperties {
                source_start,
                source_duration,
                playback_speed,
            }),
            _ => None,
        }
    }

    pub(super) fn aspect_lock(&self) -> Option<(&str, bool)> {
        match self {
            Self::AspectLock { property, default } => Some((property, *default)),
            _ => None,
        }
    }

    fn validate(
        &self,
        owner: &str,
        id: &str,
        properties: &[PropertySchema],
    ) -> Result<(), PluginError> {
        if owner != "item" && matches!(self, Self::Timeline { .. } | Self::Label { .. }) {
            return Err(PluginError::invalid_definition(format!(
                "{} editor is only supported for items",
                self.kind()
            )));
        }
        let context = format!("{owner} '{id}' editor {}", self.kind());
        let references = PropertyReferences::new(&context, properties);
        let check_origin = |id: &Option<String>| {
            id.as_deref()
                .map_or(Ok(()), |id| references.origin(id).map(|_| ()))
        };
        let points = |property: &str, position: &str, size: &str| {
            references.check(
                property,
                "an array of two-f32 tuples",
                |property| matches!(property.definition(), PropertyDefinition::Array { element, .. } if is_f32_pair(element)),
            )?;
            references.pair(position)?;
            references.pair(size).map(|_| ())
        };
        match self {
            Self::Timeline {
                source_start,
                source_duration,
                playback_speed,
            } => TimeMappingProperties {
                source_start,
                source_duration,
                playback_speed,
            }
            .validate(owner, id, properties),
            Self::Position { property } | Self::AspectLock { property, .. } => {
                references.pair(property).map(|_| ())
            }
            Self::Size {
                property,
                position,
                origin,
            } => {
                references.pair(property)?;
                references.pair(position)?;
                check_origin(origin)
            }
            Self::Points {
                property,
                position,
                size,
                origin,
            } => {
                points(property, position, size)?;
                check_origin(origin)
            }
            Self::Spline {
                points: property,
                position,
                size,
                origin,
                tension,
                closed,
            } => {
                points(property, position, size)?;
                check_origin(origin)?;
                references.scalar(tension, ScalarPropertyType::F32)?;
                references
                    .scalar(closed, ScalarPropertyType::Bool)
                    .map(|_| ())
            }
            Self::Label { property } => references
                .scalar(property, ScalarPropertyType::String)
                .map(|_| ()),
        }
    }
}

pub(super) fn validate_editors(
    owner: &str,
    id: &str,
    properties: &[PropertySchema],
    editors: &[EditorCapability],
) -> Result<(), PluginError> {
    let mut kinds = HashSet::new();
    for editor in editors {
        editor.validate(owner, id, properties)?;
        let kind = editor.kind();
        if !kinds.insert(kind) {
            return Err(PluginError::invalid_definition(format!(
                "{owner} '{id}' has duplicate {kind} editors"
            )));
        }
    }
    Ok(())
}
