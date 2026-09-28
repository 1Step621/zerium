//! Plugin declarations for the logical area produced by a visual operation.

use serde::Deserialize;

use super::PluginError;
use crate::domain::property::{
    PropertySchema, PropertyType, PropertyValueType, ScalarPropertyType,
};

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum ItemBoundsSchema {
    Viewport,
    Quad {
        position: String,
        size: String,
        #[serde(default)]
        rotation: Option<String>,
    },
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum OutputBoundsSchema {
    #[default]
    Same,
    Viewport,
    Translate {
        offset: String,
    },
    Outset {
        radius: String,
        multiplier: f32,
    },
    Rotate {
        angle: String,
        center: String,
    },
    Perspective {
        rotation: String,
        center: String,
        perspective: String,
    },
}

fn is_f32_pair(property: &PropertySchema) -> bool {
    matches!(
        property.ty(),
        PropertyType::Value(PropertyValueType::Tuple(tuple))
            if tuple.scalars() == [ScalarPropertyType::F32, ScalarPropertyType::F32]
    )
}

fn is_f32(property: &PropertySchema) -> bool {
    matches!(
        property.ty(),
        PropertyType::Value(PropertyValueType::Scalar(ScalarPropertyType::F32))
    )
}

fn is_f32_triple(property: &PropertySchema) -> bool {
    matches!(
        property.ty(),
        PropertyType::Value(PropertyValueType::Tuple(tuple))
            if tuple.scalars() == [ScalarPropertyType::F32, ScalarPropertyType::F32, ScalarPropertyType::F32]
    )
}

fn validate_reference(
    owner: &str,
    id: &str,
    property: &str,
    properties: &[PropertySchema],
    pair: bool,
) -> Result<(), PluginError> {
    let valid = properties
        .iter()
        .find(|candidate| candidate.id() == property)
        .is_some_and(|candidate| {
            if pair {
                is_f32_pair(candidate)
            } else {
                is_f32(candidate)
            }
        });
    if valid {
        Ok(())
    } else {
        Err(PluginError::invalid_definition(format!(
            "{owner} '{id}' bounds property '{property}' must be {}",
            if pair {
                "a pair of f32 values"
            } else {
                "an f32 value"
            }
        )))
    }
}

impl ItemBoundsSchema {
    pub(super) fn validate(
        &self,
        id: &str,
        properties: &[PropertySchema],
    ) -> Result<(), PluginError> {
        match self {
            Self::Viewport => {}
            Self::Quad {
                position,
                size,
                rotation,
            } => {
                validate_reference("item", id, position, properties, true)?;
                validate_reference("item", id, size, properties, true)?;
                if let Some(rotation) = rotation {
                    validate_reference("item", id, rotation, properties, false)?;
                }
            }
        }
        Ok(())
    }
}

impl OutputBoundsSchema {
    pub(super) fn validate(
        &self,
        id: &str,
        properties: &[PropertySchema],
    ) -> Result<(), PluginError> {
        match self {
            Self::Same | Self::Viewport => {}
            Self::Translate { offset } => {
                validate_reference("effect", id, offset, properties, true)?
            }
            Self::Outset { radius, multiplier } => {
                validate_reference("effect", id, radius, properties, false)?;
                if !multiplier.is_finite() || *multiplier < 0.0 {
                    return Err(PluginError::invalid_definition(format!(
                        "effect '{id}' bounds multiplier must be finite and non-negative"
                    )));
                }
            }
            Self::Rotate { angle, center } => {
                validate_reference("effect", id, angle, properties, false)?;
                validate_reference("effect", id, center, properties, true)?;
            }
            Self::Perspective {
                rotation,
                center,
                perspective,
            } => {
                let valid = properties
                    .iter()
                    .find(|candidate| candidate.id() == rotation)
                    .is_some_and(is_f32_triple);
                if !valid {
                    return Err(PluginError::invalid_definition(format!(
                        "effect '{id}' bounds property '{rotation}' must be three f32 values"
                    )));
                }
                validate_reference("effect", id, center, properties, true)?;
                validate_reference("effect", id, perspective, properties, false)?;
            }
        }
        Ok(())
    }
}
