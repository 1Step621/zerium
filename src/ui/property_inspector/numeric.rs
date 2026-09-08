use super::*;

#[derive(Clone)]
pub(super) struct NumericInput {
    pub(super) scalar: ScalarParameterType,
    pub(super) scale: f64,
}

impl NumericInput {
    pub(super) fn new(scalar: ScalarParameterType, scale: f64) -> Option<Self> {
        (matches!(
            scalar,
            ScalarParameterType::F32 | ScalarParameterType::I32 | ScalarParameterType::U32
        ) && scale.is_finite()
            && scale > 0.)
            .then_some(Self { scalar, scale })
    }

    pub(super) fn for_schema(schema: &ParameterSchema) -> Option<Self> {
        Self::new(
            schema.ty().scalar_type()?.clone(),
            f64::from(schema.ui().display_scale()),
        )
    }

    /// Floats use the caller's display-unit step; integers advance one stored unit.
    pub(super) fn step(&self, float_display_step: f64) -> f64 {
        if self.scalar == ScalarParameterType::F32 {
            float_display_step
        } else {
            self.scale
        }
    }

    pub(super) fn format(&self, value: f64) -> String {
        if self.scalar == ScalarParameterType::F32 && (value as f32).is_finite() {
            (value as f32).to_string()
        } else {
            value.to_string()
        }
    }

    pub(super) fn bounds(&self) -> (f64, f64) {
        match self.scalar {
            ScalarParameterType::I32 => (f64::from(i32::MIN), f64::from(i32::MAX)),
            ScalarParameterType::U32 => (0., f64::from(u32::MAX)),
            _ => (f64::from(f32::MIN), f64::from(f32::MAX)),
        }
    }

    pub(super) fn parse_optional(&self, text: &str) -> Option<Option<ParameterValue>> {
        if text.trim().is_empty() {
            Some(None)
        } else {
            self.parse(text).map(Some)
        }
    }

    pub(super) fn parse(&self, text: &str) -> Option<ParameterValue> {
        if self.scale == 1. {
            match self.scalar {
                ScalarParameterType::I32 => {
                    return text.trim().parse::<i32>().ok().map(ParameterValue::I32);
                }
                ScalarParameterType::U32 => {
                    return text.trim().parse::<u32>().ok().map(ParameterValue::U32);
                }
                _ => {}
            }
        }
        let value = text.trim().parse::<f64>().ok()? / self.scale;
        if !value.is_finite() {
            return None;
        }
        match self.scalar {
            ScalarParameterType::F32 if value.abs() <= f64::from(f32::MAX) => {
                Some(ParameterValue::F32(value as f32))
            }
            ScalarParameterType::I32
                if value.fract() == 0.
                    && (f64::from(i32::MIN)..=f64::from(i32::MAX)).contains(&value) =>
            {
                Some(ParameterValue::I32(value as i32))
            }
            ScalarParameterType::U32
                if value.fract() == 0. && (0. ..=f64::from(u32::MAX)).contains(&value) =>
            {
                Some(ParameterValue::U32(value as u32))
            }
            _ => None,
        }
    }
}
