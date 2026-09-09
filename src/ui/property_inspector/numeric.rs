use super::*;

#[derive(Clone)]
pub(super) struct NumericInput {
    pub(super) scalar: ScalarParameterType,
}

impl NumericInput {
    pub(super) fn new(scalar: ScalarParameterType) -> Option<Self> {
        matches!(
            scalar,
            ScalarParameterType::F32 | ScalarParameterType::I32 | ScalarParameterType::U32
        )
        .then_some(Self { scalar })
    }

    pub(super) fn for_schema(schema: &ParameterSchema) -> Option<Self> {
        Self::new(schema.ty().scalar_type()?.clone())
    }

    pub(super) fn step(&self, float_step: f64) -> f64 {
        if self.scalar == ScalarParameterType::F32 {
            float_step
        } else {
            1.
        }
    }

    pub(super) fn format(&self, value: f64) -> String {
        if self.scalar == ScalarParameterType::F32 && (value as f32).is_finite() {
            (value as f32).to_string()
        } else {
            value.to_string()
        }
    }

    pub(super) fn value_from_number(&self, value: f64) -> Option<ParameterValue> {
        self.parse(&self.format(value))
    }

    pub(super) fn parse_number(&self, text: &str) -> Option<f64> {
        self.parse(text).and_then(|value| value.numeric_scalar())
    }

    pub(super) fn bounds(&self) -> (f64, f64) {
        match self.scalar {
            ScalarParameterType::I32 => (f64::from(i32::MIN), f64::from(i32::MAX)),
            ScalarParameterType::U32 => (0., f64::from(u32::MAX)),
            _ => (f64::from(f32::MIN), f64::from(f32::MAX)),
        }
    }

    pub(super) fn parse(&self, text: &str) -> Option<ParameterValue> {
        let value = text.trim().parse::<f64>().ok()?;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numeric_input_uses_canonical_values() {
        let number = NumericInput::new(ScalarParameterType::F32).unwrap();

        assert_eq!(number.parse_number("25"), Some(25.));
        assert_eq!(number.format(0.25), "0.25");
        assert_eq!(
            number.value_from_number(25.),
            Some(ParameterValue::F32(25.))
        );
    }
}
