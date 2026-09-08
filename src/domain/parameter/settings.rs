//! Validated numeric defaults and optional bounds, independent of the editor.

use super::{ParameterConstraints, ParameterValue};

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct NumberSettings<T> {
    default: T,
    min: Option<T>,
    max: Option<T>,
}

impl<T: Copy + PartialOrd + Into<f64>> NumberSettings<T> {
    pub(crate) fn new(default: T, min: Option<T>, max: Option<T>) -> Option<Self> {
        if !default.into().is_finite()
            || min.is_some_and(|value| !value.into().is_finite())
            || max.is_some_and(|value| !value.into().is_finite())
            || matches!((min, max), (Some(min), Some(max)) if min > max)
        {
            return None;
        }
        let default = min.filter(|min| default < *min).unwrap_or(default);
        let default = max.filter(|max| default > *max).unwrap_or(default);
        Some(Self { default, min, max })
    }

    fn constraints(&self) -> ParameterConstraints {
        ParameterConstraints::from_bounds(self.min.map(Into::into), self.max.map(Into::into))
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum NumericSettings {
    F32(NumberSettings<f32>),
    I32(NumberSettings<i32>),
    U32(NumberSettings<u32>),
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
                NumberSettings::new($default, min, max).map(Self::$variant)
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
        match self {
            Self::F32(settings) => (
                ParameterValue::F32(settings.default),
                settings.constraints(),
            ),
            Self::I32(settings) => (
                ParameterValue::I32(settings.default),
                settings.constraints(),
            ),
            Self::U32(settings) => (
                ParameterValue::U32(settings.default),
                settings.constraints(),
            ),
        }
    }
}
