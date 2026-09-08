//! Type-aware numeric constraint algebra for parameter defaults and bindings.

use serde::{Deserialize, Serialize};

use super::ParameterError;
use crate::domain::parameter::{
    ParameterType, ParameterValue, ParameterValueType, ScalarParameterType,
};

/// Wire bounds use f64 so every i32/u32 endpoint is represented exactly.
#[derive(Clone, Debug, Default, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ParameterConstraints {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max: Option<f64>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    elements: Vec<ParameterConstraints>,
}

impl ParameterConstraints {
    pub(crate) fn from_bounds(min: Option<f64>, max: Option<f64>) -> Self {
        Self {
            min,
            max,
            elements: Vec::new(),
        }
    }

    pub(crate) fn for_element(&self, element: usize) -> &Self {
        static DEFAULT: ParameterConstraints = ParameterConstraints {
            min: None,
            max: None,
            elements: Vec::new(),
        };
        self.elements.get(element).unwrap_or(&DEFAULT)
    }
    pub(super) fn intersection(&self, other: &Self) -> Option<Self> {
        if !self.elements.is_empty() || !other.elements.is_empty() {
            let count = self.elements.len().max(other.elements.len());
            if (!self.elements.is_empty() && self.elements.len() != count)
                || (!other.elements.is_empty() && other.elements.len() != count)
            {
                return None;
            }
            let elements = (0..count)
                .map(|index| {
                    self.for_element(index)
                        .intersection(other.for_element(index))
                })
                .collect::<Option<_>>()?;
            return Some(Self {
                elements,
                ..Self::default()
            });
        }
        let min = match (self.min, other.min) {
            (Some(left), Some(right)) => Some(left.max(right)),
            (left, right) => left.or(right),
        };
        let max = match (self.max, other.max) {
            (Some(left), Some(right)) => Some(left.min(right)),
            (left, right) => left.or(right),
        };
        (!matches!((min, max), (Some(min), Some(max)) if min > max))
            .then(|| Self::from_bounds(min, max))
    }

    pub(super) fn contains(&self, other: &Self) -> bool {
        if !self.elements.is_empty() || !other.elements.is_empty() {
            let count = self.elements.len().max(other.elements.len());
            if (!self.elements.is_empty() && self.elements.len() != count)
                || (!other.elements.is_empty() && other.elements.len() != count)
            {
                return false;
            }
            return (0..count)
                .all(|index| self.for_element(index).contains(other.for_element(index)));
        }
        self.min.is_none_or(|minimum| {
            other
                .min
                .is_some_and(|other_minimum| other_minimum >= minimum)
        }) && self.max.is_none_or(|maximum| {
            other
                .max
                .is_some_and(|other_maximum| other_maximum <= maximum)
        })
    }

    pub(in crate::domain) fn allows(&self, value: &ParameterValue) -> bool {
        match value {
            ParameterValue::F32(value) => self.allows_number(f64::from(*value)),
            ParameterValue::I32(value) => self.allows_number(f64::from(*value)),
            ParameterValue::U32(value) => self.allows_number(f64::from(*value)),
            ParameterValue::Tuple(values) => values
                .iter()
                .enumerate()
                .all(|(index, value)| self.for_element(index).allows(value)),
            ParameterValue::Array(values) => values.iter().all(|value| self.allows(value)),
            ParameterValue::Color(values) => values
                .iter()
                .all(|value| self.allows_number(f64::from(*value))),
            ParameterValue::Bool(_) | ParameterValue::String(_) | ParameterValue::Enum(_) => {
                self.min.is_none() && self.max.is_none()
            }
        }
    }

    fn allows_number(&self, value: f64) -> bool {
        value.is_finite()
            && !self.min.is_some_and(|min| value < min)
            && !self.max.is_some_and(|max| value > max)
    }

    pub(crate) fn clamp_value(&self, value: &ParameterValue) -> Option<ParameterValue> {
        match value {
            ParameterValue::F32(value) => {
                let value = self.clamp_f64(f64::from(*value));
                let value = value as f32;
                value.is_finite().then_some(ParameterValue::F32(value))
            }
            ParameterValue::I32(value) => Some(ParameterValue::I32(self.clamp_i32(*value)?)),
            ParameterValue::U32(value) => Some(ParameterValue::U32(self.clamp_u32(*value)?)),
            ParameterValue::Tuple(values) => values
                .iter()
                .enumerate()
                .map(|(index, value)| self.for_element(index).clamp_value(value))
                .collect::<Option<Vec<_>>>()
                .map(ParameterValue::Tuple),
            ParameterValue::Array(values) => values
                .iter()
                .map(|value| self.clamp_value(value))
                .collect::<Option<Vec<_>>>()
                .map(ParameterValue::Array),
            ParameterValue::Color(values) => {
                let mut constrained = *values;
                for value in &mut constrained {
                    *value = self.clamp_f64(f64::from(*value)) as f32;
                    if !value.is_finite() {
                        return None;
                    }
                }
                Some(ParameterValue::Color(constrained))
            }
            ParameterValue::Enum(value) if self.min.is_none() && self.max.is_none() => {
                Some(ParameterValue::Enum(*value))
            }
            ParameterValue::Bool(value) if self.min.is_none() && self.max.is_none() => {
                Some(ParameterValue::Bool(*value))
            }
            ParameterValue::String(value) if self.min.is_none() && self.max.is_none() => {
                Some(ParameterValue::String(value.clone()))
            }
            ParameterValue::Bool(_) | ParameterValue::String(_) | ParameterValue::Enum(_) => None,
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
        parameter_id: &str,
        ty: &ParameterType,
        default: &ParameterValue,
    ) -> Result<(), ParameterError> {
        let invalid = || {
            ParameterError::invalid_definition(format!(
                "{owner_kind} '{owner_id}' parameter '{parameter_id}' has invalid constraints"
            ))
        };
        if !self.bounds_valid() || !self.valid_for_type(ty) {
            return Err(invalid());
        }
        if default.matches_type(ty) && !self.allows(default) {
            return Err(ParameterError::invalid_definition(format!(
                "{owner_kind} '{owner_id}' parameter '{parameter_id}' default violates its constraints"
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

    fn valid_for_type(&self, ty: &ParameterType) -> bool {
        if !self.elements.is_empty() {
            let ParameterValueType::Tuple(tuple) = ty.element_type() else {
                return false;
            };
            return self.min.is_none()
                && self.max.is_none()
                && self.elements.len() == tuple.element_count()
                && self
                    .elements
                    .iter()
                    .zip(tuple.elements())
                    .all(|(constraints, ty)| {
                        constraints.elements.is_empty()
                            && constraints.bounds_valid()
                            && ((constraints.min.is_none() && constraints.max.is_none())
                                || constraints.valid_for_scalar(ty))
                    });
        }
        let constrained = self.min.is_some() || self.max.is_some();
        if !constrained {
            return true;
        }
        match ty.element_type() {
            ParameterValueType::Scalar(ty) => self.valid_for_scalar(ty),
            ParameterValueType::Tuple(_) => false,
        }
    }

    fn valid_for_scalar(&self, ty: &ScalarParameterType) -> bool {
        match ty {
            ScalarParameterType::F32 | ScalarParameterType::Color => self
                .min
                .into_iter()
                .chain(self.max)
                .all(|value| (value as f32).is_finite()),
            ScalarParameterType::I32 => {
                let lower = self.min.unwrap_or(f64::from(i32::MIN)).ceil();
                let upper = self.max.unwrap_or(f64::from(i32::MAX)).floor();
                lower <= upper && upper >= f64::from(i32::MIN) && lower <= f64::from(i32::MAX)
            }
            ScalarParameterType::U32 => {
                let lower = self.min.unwrap_or(0.0).max(0.0).ceil();
                let upper = self
                    .max
                    .unwrap_or(f64::from(u32::MAX))
                    .min(f64::from(u32::MAX))
                    .floor();
                lower <= upper
            }
            ScalarParameterType::Bool
            | ScalarParameterType::String
            | ScalarParameterType::Enum(_) => false,
        }
    }
}
