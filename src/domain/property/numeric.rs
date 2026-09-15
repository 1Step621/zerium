//! Numeric value conversion and validated editor settings.

use super::{PropertyConstraints, PropertyValue};

impl PropertyValue {
    pub(crate) fn numeric_scalar(&self) -> Option<f64> {
        match self {
            Self::F32(value) => Some(f64::from(*value)),
            Self::I32(value) => Some(f64::from(*value)),
            Self::U32(value) => Some(f64::from(*value)),
            _ => None,
        }
    }

    pub(crate) fn with_numeric_scalar(&self, value: f64) -> Option<Self> {
        if !value.is_finite() {
            return None;
        }
        match self {
            Self::F32(_) if value.abs() <= f64::from(f32::MAX) => Some(Self::F32(value as f32)),
            Self::I32(_) if (f64::from(i32::MIN)..=f64::from(i32::MAX)).contains(&value) => {
                Some(Self::I32(value.round() as i32))
            }
            Self::U32(_) if (0. ..=f64::from(u32::MAX)).contains(&value) => {
                Some(Self::U32(value.round() as u32))
            }
            _ => None,
        }
    }
}

fn normalize_numeric_settings<T: Copy + PartialOrd + Into<f64>>(
    default: T,
    min: Option<T>,
    max: Option<T>,
) -> Option<(T, PropertyConstraints)> {
    if !default.into().is_finite()
        || min.is_some_and(|value| !value.into().is_finite())
        || max.is_some_and(|value| !value.into().is_finite())
        || matches!((min, max), (Some(min), Some(max)) if min > max)
    {
        return None;
    }
    let default = min.filter(|min| default < *min).unwrap_or(default);
    let default = max.filter(|max| default > *max).unwrap_or(default);
    let constraints = PropertyConstraints::from_bounds(min.map(Into::into), max.map(Into::into));
    Some((default, constraints))
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct NumericSettings {
    default: PropertyValue,
    constraints: PropertyConstraints,
}

impl NumericSettings {
    /// Validate dynamically typed input once, before it enters an editing command.
    pub(crate) fn from_values(
        default: PropertyValue,
        min: Option<PropertyValue>,
        max: Option<PropertyValue>,
    ) -> Option<Self> {
        macro_rules! settings {
            ($variant:ident, $default:expr) => {{
                let min = match min {
                    Some(PropertyValue::$variant(value)) => Some(value),
                    None => None,
                    _ => return None,
                };
                let max = match max {
                    Some(PropertyValue::$variant(value)) => Some(value),
                    None => None,
                    _ => return None,
                };
                let (default, constraints) = normalize_numeric_settings($default, min, max)?;
                Some(Self {
                    default: PropertyValue::$variant(default),
                    constraints,
                })
            }};
        }
        match default {
            PropertyValue::F32(value) => settings!(F32, value),
            PropertyValue::I32(value) => settings!(I32, value),
            PropertyValue::U32(value) => settings!(U32, value),
            _ => None,
        }
    }

    pub(in crate::domain) fn into_parts(self) -> (PropertyValue, PropertyConstraints) {
        (self.default, self.constraints)
    }
}
