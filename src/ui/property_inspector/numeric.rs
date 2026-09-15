use super::*;

#[derive(Clone)]
pub(super) struct NumericInput {
    pub(super) scalar: ScalarPropertyType,
}

impl NumericInput {
    pub(super) fn new(scalar: ScalarPropertyType) -> Option<Self> {
        matches!(
            scalar,
            ScalarPropertyType::F32 | ScalarPropertyType::I32 | ScalarPropertyType::U32
        )
        .then_some(Self { scalar })
    }

    pub(super) fn for_schema(schema: &PropertySchema) -> Option<Self> {
        let PropertyType::Value(value_type) = schema.ty() else {
            return None;
        };
        Self::new(value_type.scalar_at(None)?.clone())
    }

    pub(super) fn step(&self, float_step: f64) -> f64 {
        if self.scalar == ScalarPropertyType::F32 {
            float_step
        } else {
            1.
        }
    }

    pub(super) fn format(&self, value: f64) -> String {
        if self.scalar == ScalarPropertyType::F32 && (value as f32).is_finite() {
            (value as f32).to_string()
        } else {
            value.to_string()
        }
    }

    pub(super) fn value_from_number(&self, value: f64) -> Option<PropertyValue> {
        self.parse(&self.format(value))
    }

    pub(super) fn parse_number(&self, text: &str) -> Option<f64> {
        self.parse(text).and_then(|value| value.numeric_scalar())
    }

    pub(super) fn bounds(&self) -> (f64, f64) {
        match self.scalar {
            ScalarPropertyType::I32 => (f64::from(i32::MIN), f64::from(i32::MAX)),
            ScalarPropertyType::U32 => (0., f64::from(u32::MAX)),
            _ => (f64::from(f32::MIN), f64::from(f32::MAX)),
        }
    }

    pub(super) fn parse(&self, text: &str) -> Option<PropertyValue> {
        let value = text.trim().parse::<f64>().ok()?;
        if !value.is_finite() {
            return None;
        }
        match self.scalar {
            ScalarPropertyType::F32 if value.abs() <= f64::from(f32::MAX) => {
                Some(PropertyValue::F32(value as f32))
            }
            ScalarPropertyType::I32
                if value.fract() == 0.
                    && (f64::from(i32::MIN)..=f64::from(i32::MAX)).contains(&value) =>
            {
                Some(PropertyValue::I32(value as i32))
            }
            ScalarPropertyType::U32
                if value.fract() == 0. && (0. ..=f64::from(u32::MAX)).contains(&value) =>
            {
                Some(PropertyValue::U32(value as u32))
            }
            _ => None,
        }
    }
}
