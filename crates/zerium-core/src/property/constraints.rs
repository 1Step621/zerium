//! Type-aware numeric constraint algebra for property defaults and bindings.

use serde::{Deserialize, Serialize};

use super::PropertyError;
use crate::property::{PropertyType, PropertyValue, PropertyValueType, ScalarPropertyType};

/// Wire bounds use f64 so every i32/u32 endpoint is represented exactly.
#[derive(Clone, Debug, Default, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PropertyConstraints {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max: Option<f64>,
}

impl PropertyConstraints {
    pub fn is_default(&self) -> bool {
        self == &Self::default()
    }

    pub fn from_bounds(min: Option<f64>, max: Option<f64>) -> Self {
        Self { min, max }
    }

    pub(crate) fn allows(&self, value: &PropertyValue) -> bool {
        match value {
            PropertyValue::F32(value) => self.allows_number(f64::from(*value)),
            PropertyValue::I32(value) => self.allows_number(f64::from(*value)),
            PropertyValue::U32(value) => self.allows_number(f64::from(*value)),
            PropertyValue::Tuple(_) | PropertyValue::Array(_) => false,
            PropertyValue::Color(values) => values
                .iter()
                .all(|value| self.allows_number(f64::from(*value))),
            PropertyValue::File(_)
            | PropertyValue::Bool(_)
            | PropertyValue::String(_)
            | PropertyValue::Enum(_) => self.min.is_none() && self.max.is_none(),
        }
    }

    fn allows_number(&self, value: f64) -> bool {
        value.is_finite()
            && !self.min.is_some_and(|min| value < min)
            && !self.max.is_some_and(|max| value > max)
    }

    /// The representable numeric interval inside the declared wire bounds.
    pub fn numeric_bounds(&self, ty: &ScalarPropertyType) -> Option<(f64, f64)> {
        if !self.bounds_valid() {
            return None;
        }
        let (native_min, native_max) = match ty {
            ScalarPropertyType::F32 | ScalarPropertyType::Color => {
                if self
                    .min
                    .into_iter()
                    .chain(self.max)
                    .any(|value| value.abs() > f64::from(f32::MAX))
                {
                    return None;
                }
                (f64::from(f32::MIN), f64::from(f32::MAX))
            }
            ScalarPropertyType::I32 => (f64::from(i32::MIN), f64::from(i32::MAX)),
            ScalarPropertyType::U32 => (0., f64::from(u32::MAX)),
            _ => return None,
        };
        let min = self.min.unwrap_or(native_min).max(native_min);
        let max = self.max.unwrap_or(native_max).min(native_max);
        let (min, max) = match ty {
            ScalarPropertyType::F32 | ScalarPropertyType::Color => {
                let lower = min as f32;
                let upper = max as f32;
                (
                    f64::from(if f64::from(lower) < min {
                        lower.next_up()
                    } else {
                        lower
                    }),
                    f64::from(if f64::from(upper) > max {
                        upper.next_down()
                    } else {
                        upper
                    }),
                )
            }
            _ => (min.ceil(), max.floor()),
        };
        (min <= max).then_some((min, max))
    }

    pub fn clamp_value(&self, value: &PropertyValue) -> Option<PropertyValue> {
        match value {
            PropertyValue::F32(value) => {
                self.clamp_number(f64::from(*value), ScalarPropertyType::F32)
            }
            PropertyValue::I32(value) => {
                self.clamp_number(f64::from(*value), ScalarPropertyType::I32)
            }
            PropertyValue::U32(value) => {
                self.clamp_number(f64::from(*value), ScalarPropertyType::U32)
            }
            PropertyValue::Tuple(_) | PropertyValue::Array(_) => None,
            PropertyValue::Color(values) => {
                if !values.iter().all(|value| value.is_finite()) {
                    return None;
                }
                let (min, max) = self.numeric_bounds(&ScalarPropertyType::Color)?;
                Some(PropertyValue::Color(
                    values.map(|value| f64::from(value).clamp(min, max) as f32),
                ))
            }
            PropertyValue::File(_)
            | PropertyValue::Enum(_)
            | PropertyValue::Bool(_)
            | PropertyValue::String(_)
                if self.min.is_none() && self.max.is_none() =>
            {
                Some(value.clone())
            }
            _ => None,
        }
    }

    fn clamp_number(&self, value: f64, ty: ScalarPropertyType) -> Option<PropertyValue> {
        if !value.is_finite() {
            return None;
        }
        let (min, max) = self.numeric_bounds(&ty)?;
        ty.value_from_number(value.clamp(min, max))
    }

    pub(super) fn validate(
        &self,
        owner_kind: &str,
        owner_id: &str,
        property_id: &str,
        ty: &PropertyType,
        default: Option<&PropertyValue>,
    ) -> Result<(), PropertyError> {
        let invalid = || {
            PropertyError::invalid_definition(format!(
                "{owner_kind} '{owner_id}' property '{property_id}' has invalid constraints"
            ))
        };
        if !self.bounds_valid() || !self.valid_for_type(ty) {
            return Err(invalid());
        }
        if default.is_some_and(|default| ty.allows(default) && !self.allows(default)) {
            return Err(PropertyError::invalid_definition(format!(
                "{owner_kind} '{owner_id}' property '{property_id}' default violates its constraints"
            )));
        }
        Ok(())
    }

    fn bounds_valid(&self) -> bool {
        let min = self.min;
        let max = self.max;
        min.is_none_or(f64::is_finite)
            && max.is_none_or(f64::is_finite)
            && !matches!((min, max), (Some(min), Some(max)) if min > max)
    }

    fn valid_for_type(&self, ty: &PropertyType) -> bool {
        let value_type = ty.value_type();
        let constrained = self.min.is_some() || self.max.is_some();
        if !constrained {
            return true;
        }
        match value_type {
            Some(PropertyValueType::Scalar(ty)) => self.valid_for_scalar(ty),
            Some(PropertyValueType::Tuple(_)) | None => false,
        }
    }

    fn valid_for_scalar(&self, ty: &ScalarPropertyType) -> bool {
        self.numeric_bounds(ty).is_some()
    }
}
