//! Numeric value conversion and validated editor settings.

use super::{ParameterConstraints, ParameterValue};

impl ParameterValue {
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
) -> Option<(T, ParameterConstraints)> {
    if !default.into().is_finite()
        || min.is_some_and(|value| !value.into().is_finite())
        || max.is_some_and(|value| !value.into().is_finite())
        || matches!((min, max), (Some(min), Some(max)) if min > max)
    {
        return None;
    }
    let default = min.filter(|min| default < *min).unwrap_or(default);
    let default = max.filter(|max| default > *max).unwrap_or(default);
    let constraints = ParameterConstraints::from_bounds(min.map(Into::into), max.map(Into::into));
    Some((default, constraints))
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct NumericSettings {
    default: ParameterValue,
    constraints: ParameterConstraints,
}

impl NumericSettings {
    /// Validate dynamically typed input once, before it enters an editing command.
    pub(crate) fn from_values(
        default: ParameterValue,
        min: Option<ParameterValue>,
        max: Option<ParameterValue>,
    ) -> Option<Self> {
        macro_rules! settings {
            ($variant:ident, $default:expr) => {{
                let min = match min {
                    Some(ParameterValue::$variant(value)) => Some(value),
                    None => None,
                    _ => return None,
                };
                let max = match max {
                    Some(ParameterValue::$variant(value)) => Some(value),
                    None => None,
                    _ => return None,
                };
                let (default, constraints) = normalize_numeric_settings($default, min, max)?;
                Some(Self {
                    default: ParameterValue::$variant(default),
                    constraints,
                })
            }};
        }
        match default {
            ParameterValue::F32(value) => settings!(F32, value),
            ParameterValue::I32(value) => settings!(I32, value),
            ParameterValue::U32(value) => settings!(U32, value),
            _ => None,
        }
    }

    pub(in crate::domain) fn into_parts(self) -> (ParameterValue, ParameterConstraints) {
        (self.default, self.constraints)
    }
}
