//! Type-aware numeric constraint algebra for property defaults and bindings.

use serde::{Deserialize, Serialize};

use super::PropertyError;
use crate::domain::property::{PropertyType, PropertyValue, PropertyValueType, ScalarPropertyType};

/// Wire bounds use f64 so every i32/u32 endpoint is represented exactly.
#[derive(Clone, Debug, Default, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PropertyConstraints {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max: Option<f64>,
}

impl PropertyConstraints {
    pub(crate) fn is_default(&self) -> bool {
        self == &Self::default()
    }

    pub(crate) fn from_bounds(min: Option<f64>, max: Option<f64>) -> Self {
        Self { min, max }
    }

    pub(in crate::domain) fn allows(&self, value: &PropertyValue) -> bool {
        match value {
            PropertyValue::F32(value) => self.allows_number(f64::from(*value)),
            PropertyValue::I32(value) => self.allows_number(f64::from(*value)),
            PropertyValue::U32(value) => self.allows_number(f64::from(*value)),
            PropertyValue::Tuple(_) | PropertyValue::Array(_) => false,
            PropertyValue::Color(values) => values
                .iter()
                .all(|value| self.allows_number(f64::from(*value))),
            PropertyValue::Bool(_) | PropertyValue::String(_) | PropertyValue::Enum(_) => {
                self.min.is_none() && self.max.is_none()
            }
        }
    }

    fn allows_number(&self, value: f64) -> bool {
        value.is_finite()
            && !self.min.is_some_and(|min| value < min)
            && !self.max.is_some_and(|max| value > max)
    }

    pub(crate) fn clamp_value(&self, value: &PropertyValue) -> Option<PropertyValue> {
        match value {
            PropertyValue::F32(value) => {
                let value = self.clamp_f64(f64::from(*value));
                let value = value as f32;
                value.is_finite().then_some(PropertyValue::F32(value))
            }
            PropertyValue::I32(value) => Some(PropertyValue::I32(self.clamp_i32(*value)?)),
            PropertyValue::U32(value) => Some(PropertyValue::U32(self.clamp_u32(*value)?)),
            PropertyValue::Tuple(_) | PropertyValue::Array(_) => None,
            PropertyValue::Color(values) => {
                let mut constrained = *values;
                for value in &mut constrained {
                    *value = self.clamp_f64(f64::from(*value)) as f32;
                    if !value.is_finite() {
                        return None;
                    }
                }
                Some(PropertyValue::Color(constrained))
            }
            PropertyValue::Enum(value) if self.min.is_none() && self.max.is_none() => {
                Some(PropertyValue::Enum(*value))
            }
            PropertyValue::Bool(value) if self.min.is_none() && self.max.is_none() => {
                Some(PropertyValue::Bool(*value))
            }
            PropertyValue::String(value) if self.min.is_none() && self.max.is_none() => {
                Some(PropertyValue::String(value.clone()))
            }
            PropertyValue::Bool(_) | PropertyValue::String(_) | PropertyValue::Enum(_) => None,
        }
    }

    fn clamp_f64(&self, value: f64) -> f64 {
        let value = self.min.map_or(value, |min| value.max(min));
        self.max.map_or(value, |max| value.min(max))
    }

    fn clamp_i32(&self, value: i32) -> Option<i32> {
        let mut value = i64::from(value);
        if let Some(min) = self.min
            && (value as f64) < min
        {
            value = min.ceil() as i64;
        }
        if let Some(max) = self.max
            && (value as f64) > max
        {
            value = max.floor() as i64;
        }
        i32::try_from(value).ok()
    }

    fn clamp_u32(&self, value: u32) -> Option<u32> {
        let mut value = u64::from(value);
        if let Some(min) = self.min
            && (value as f64) < min
        {
            if min > f64::from(u32::MAX) {
                return None;
            }
            value = min.max(0.0).ceil() as u64;
        }
        if let Some(max) = self.max
            && (value as f64) > max
        {
            if max < 0.0 {
                return None;
            }
            value = max.min(f64::from(u32::MAX)).floor() as u64;
        }
        u32::try_from(value).ok()
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
        let value_type = match ty {
            PropertyType::Value(value_type)
            | PropertyType::Array {
                element_type: value_type,
                ..
            } => value_type,
        };
        let constrained = self.min.is_some() || self.max.is_some();
        if !constrained {
            return true;
        }
        match value_type {
            PropertyValueType::Scalar(ty) => self.valid_for_scalar(ty),
            PropertyValueType::Tuple(_) => false,
        }
    }

    fn valid_for_scalar(&self, ty: &ScalarPropertyType) -> bool {
        match ty {
            ScalarPropertyType::F32 | ScalarPropertyType::Color => self
                .min
                .into_iter()
                .chain(self.max)
                .all(|value| (value as f32).is_finite()),
            ScalarPropertyType::I32 => {
                let lower = self.min.unwrap_or(f64::from(i32::MIN)).ceil();
                let upper = self.max.unwrap_or(f64::from(i32::MAX)).floor();
                lower <= upper && upper >= f64::from(i32::MIN) && lower <= f64::from(i32::MAX)
            }
            ScalarPropertyType::U32 => {
                let lower = self.min.unwrap_or(0.0).max(0.0).ceil();
                let upper = self
                    .max
                    .unwrap_or(f64::from(u32::MAX))
                    .min(f64::from(u32::MAX))
                    .floor();
                lower <= upper
            }
            ScalarPropertyType::Bool | ScalarPropertyType::String | ScalarPropertyType::Enum(_) => {
                false
            }
        }
    }
}
