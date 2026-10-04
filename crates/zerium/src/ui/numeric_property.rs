use ::ui::input::{NumberInputEvent, StepAction};
use zerium_core::property::{PropertySchema, PropertyType, PropertyValue, ScalarPropertyType};

pub(super) fn snap_to_step(value: f64, step: f64) -> f64 {
    if !step.is_finite() || step <= 0. {
        value
    } else {
        let scaled = value / step;
        let snapped = scaled.round() * step;
        if snapped.is_finite() { snapped } else { value }
    }
}

/// An adjustment relative to the value at mouse-down, preserving off-grid values.
#[derive(Clone)]
pub(super) struct NumericDrag {
    pub start_x: f32,
    pub start_value: f64,
    pub step: f64,
    pub sensitivity: f64,
}

impl NumericDrag {
    pub(super) fn value_at(&self, pointer_x: f32, fine: bool) -> f64 {
        let scale = if fine { 0.1 } else { 1. };
        let delta = f64::from(pointer_x - self.start_x) * self.sensitivity * scale;
        self.start_value + snap_to_step(delta, self.sensitivity.min(self.step) * scale)
    }
}

/// Numeric display rules shared by inspector controls and animation curves.
#[derive(Clone)]
pub(super) struct NumericInputSpec {
    pub(super) suffix: String,
    pub(super) min: f64,
    pub(super) max: f64,
    pub(super) step: f64,
    pub(super) drag_step: Option<f64>,
    pub(super) scalar_type: ScalarPropertyType,
}

impl NumericInputSpec {
    pub(super) fn parse_number(&self, text: &str) -> Option<f64> {
        NumericInput::new(self.scalar_type.clone())?.parse_number(text)
    }

    pub(super) fn stepped_value(&self, value: f64, event: &NumberInputEvent) -> f64 {
        let NumberInputEvent::Step { action, fine } = event;
        let step = self.adjustment_step(*fine);
        let delta = match action {
            StepAction::Increment => step,
            StepAction::Decrement => -step,
        };
        (value + delta).clamp(self.min, self.max)
    }

    pub(super) fn adjustment_step(&self, fine: bool) -> f64 {
        let step = self.step * if fine { 0.1 } else { 1. };
        match self.scalar_type {
            ScalarPropertyType::I32 | ScalarPropertyType::U32 => step.max(1.),
            _ => step,
        }
    }
}

pub(super) fn numeric_input_spec(
    property: &PropertySchema,
    scalar_index: Option<usize>,
) -> Option<NumericInputSpec> {
    let scalar_type = property.ty().value_type()?.scalar_at(scalar_index)?.clone();
    if !property.configuration_ui(scalar_index).is_visible() {
        return None;
    }
    NumericInput::new(scalar_type.clone())?;
    let (min, max) = property
        .configuration_constraints(scalar_index)
        .numeric_bounds(&scalar_type)?;
    let ui = property.configuration_ui(scalar_index);
    let step = match scalar_type {
        ScalarPropertyType::I32 | ScalarPropertyType::U32 => f64::from(ui.step()).max(1.),
        _ => f64::from(ui.step()),
    };
    Some(NumericInputSpec {
        suffix: ui.unit().to_owned(),
        min,
        max,
        step,
        drag_step: ui.drag_step().map(f64::from),
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
        if matches!(self.scalar, ScalarPropertyType::F32) {
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
        self.scalar.value_from_number(value)
    }

    pub(super) fn parse_number(&self, text: &str) -> Option<f64> {
        self.parse(text).and_then(|value| value.numeric_scalar())
    }

    pub(super) fn bounds(&self) -> (f64, f64) {
        zerium_core::property::PropertyConstraints::default()
            .numeric_bounds(&self.scalar)
            .expect("numeric input type")
    }

    pub(super) fn parse(&self, text: &str) -> Option<PropertyValue> {
        let text = text.trim();
        match self.scalar {
            // Parse directly into the declared type to avoid double rounding.
            ScalarPropertyType::F32 => {
                let value = text.parse::<f32>().ok()?;
                value.is_finite().then_some(PropertyValue::F32(value))
            }
            ScalarPropertyType::I32 | ScalarPropertyType::U32 => {
                let value: f64 = text.parse().ok()?;
                (value.fract() == 0.)
                    .then(|| self.value_from_number(value))
                    .flatten()
            }
            _ => None,
        }
    }
}
