//! Editor numeric conversion. f64 represents every i32/u32 value exactly.
use super::ParameterValue;

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
