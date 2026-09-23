use super::*;

/// Numeric display rules used by PropertyInspector input controls.
#[derive(Clone)]
pub(super) struct NumericInputSpec {
    pub suffix: String,
    pub min: f64,
    pub max: f64,
    pub step: f64,
    pub scalar_type: ScalarPropertyType,
}

pub(super) fn numeric_input_spec(
    property: &PropertySchema,
    scalar_index: Option<usize>,
) -> Option<NumericInputSpec> {
    if !property.is_visible() || !property.configuration_ui(scalar_index).is_visible() {
        return None;
    }
    let value_type = match property.ty() {
        PropertyType::Value(value_type)
        | PropertyType::Array {
            element_type: value_type,
            ..
        } => value_type,
    };
    let scalar_type = value_type.scalar_at(scalar_index)?.clone();
    let (type_min, type_max) = match &scalar_type {
        ScalarPropertyType::F32 => (f64::from(f32::MIN), f64::from(f32::MAX)),
        ScalarPropertyType::I32 => (f64::from(i32::MIN), f64::from(i32::MAX)),
        ScalarPropertyType::U32 => (0., f64::from(u32::MAX)),
        _ => return None,
    };
    let constraints = property.configuration_constraints(scalar_index);
    let ui = property.configuration_ui(scalar_index);
    let min = constraints.min.unwrap_or(type_min).max(type_min);
    let max = constraints.max.unwrap_or(type_max).min(type_max);
    let step = f64::from(ui.step());
    let (min, max, step) = match &scalar_type {
        ScalarPropertyType::I32 | ScalarPropertyType::U32 => {
            (min.ceil(), max.floor(), step.max(1.))
        }
        _ => (min, max, step),
    };
    Some(NumericInputSpec {
        suffix: ui.unit().to_owned(),
        min,
        max,
        step,
        scalar_type,
    })
}

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
