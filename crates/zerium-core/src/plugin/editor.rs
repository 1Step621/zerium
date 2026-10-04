//! Independent editor tools and their complete property references.
use std::collections::HashSet;

use serde::Deserialize;

use super::validation::{validate_origin_property, validate_property_reference};
use super::{PluginError, TimeMappingProperties};
use crate::property::{PropertySchema, PropertyType, PropertyValueType, ScalarPropertyType};

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
        let check = |property_id: &str, expected: &str, valid: &dyn Fn(&PropertySchema) -> bool| {
            validate_property_reference(&context, properties, property_id, expected, valid)
        };
        let pair = |ty: &PropertyValueType| matches!(ty, PropertyValueType::Tuple(tuple) if tuple.scalars() == [ScalarPropertyType::F32, ScalarPropertyType::F32]);
        let tuple = |id: &str| {
            check(
                id,
                "a tuple of two f32 values",
                &|property| matches!(property.ty(), PropertyType::Value(ty) if pair(ty)),
            )
        };
        let check_origin = |id: &Option<String>| {
            id.as_deref().map_or(Ok(()), |id| {
                validate_origin_property(&context, properties, id)
            })
        };
        let points = |property: &str, position: &str, size: &str| {
            check(
                property,
                "an array of two-f32 tuples",
                &|property| matches!(property.ty(), PropertyType::Array { element_type, .. } if pair(element_type)),
            )?;
            tuple(position)?;
            tuple(size)
        };
        let scalar = |id: &str, scalar_type: ScalarPropertyType| {
            check(id, &format!("a {scalar_type:?} value"), &|property| {
                property.ty()
                    == &PropertyType::Value(PropertyValueType::Scalar(scalar_type.clone()))
            })
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
            Self::Position { property } | Self::AspectLock { property, .. } => tuple(property),
            Self::Size {
                property,
                position,
                origin,
            } => {
                tuple(property)?;
                tuple(position)?;
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
                scalar(tension, ScalarPropertyType::F32)?;
                scalar(closed, ScalarPropertyType::Bool)
            }
            Self::Label { property } => scalar(property, ScalarPropertyType::String),
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
