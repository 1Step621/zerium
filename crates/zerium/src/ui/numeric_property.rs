use ::ui::input::{NumberInputEvent, StepAction};
use zerium_core::property::{PropertySchema, PropertyValue, ScalarPropertyType};

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
    pub(super) number: NumericInput,
}

impl NumericInputSpec {
    pub(super) fn parse_number(&self, text: &str) -> Option<f64> {
        self.number.parse_number(text)
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
        match self.number {
            NumericInput::I32 | NumericInput::U32 => step.max(1.),
            NumericInput::F32 => step,
        }
    }
}

pub(super) fn numeric_input_spec(
    property: &PropertySchema,
    scalar_index: Option<usize>,
) -> Option<NumericInputSpec> {
    let scalar_type = property.value_schema().scalar(scalar_index)?.ty.clone();
    if !property.configuration_ui(scalar_index).is_visible() {
        return None;
    }
    let number = NumericInput::from_scalar(&scalar_type)?;
    let (min, max) = property
        .configuration_constraints(scalar_index)
        .numeric_bounds(&scalar_type)?;
    let ui = property.configuration_ui(scalar_index);
    let step = match number {
        NumericInput::I32 | NumericInput::U32 => f64::from(ui.step()).max(1.),
        NumericInput::F32 => f64::from(ui.step()),
    };
    Some(NumericInputSpec {
        suffix: ui.unit().to_owned(),
        min,
        max,
        step,
        drag_step: ui.drag_step().map(f64::from),
        number,
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum NumericInput {
    F32,
    I32,
    U32,
}

impl NumericInput {
    pub(super) fn from_scalar(scalar: &ScalarPropertyType) -> Option<Self> {
        match scalar {
            ScalarPropertyType::F32 => Some(Self::F32),
            ScalarPropertyType::I32 => Some(Self::I32),
            ScalarPropertyType::U32 => Some(Self::U32),
            _ => None,
        }
    }

    pub(super) fn for_schema(schema: &PropertySchema) -> Option<Self> {
        Self::from_scalar(schema.scalar_type(None, None)?)
    }

    pub(super) fn step(&self, float_step: f64) -> f64 {
        if let Self::F32 = self { float_step } else { 1. }
    }

    pub(super) fn format(&self, value: f64) -> String {
        if *self == Self::F32 && (value as f32).is_finite() {
            (value as f32).to_string()
        } else {
            value.to_string()
        }
    }

    pub(super) fn value_from_number(&self, value: f64) -> Option<PropertyValue> {
        let scalar = match self {
            Self::F32 => ScalarPropertyType::F32,
            Self::I32 => ScalarPropertyType::I32,
            Self::U32 => ScalarPropertyType::U32,
        };
        scalar.value_from_number(value)
    }

    pub(super) fn parse_number(&self, text: &str) -> Option<f64> {
        self.parse(text).and_then(|value| value.numeric_scalar())
    }

    pub(super) fn bounds(&self) -> (f64, f64) {
        match self {
            Self::F32 => (f64::from(f32::MIN), f64::from(f32::MAX)),
            Self::I32 => (f64::from(i32::MIN), f64::from(i32::MAX)),
            Self::U32 => (0., f64::from(u32::MAX)),
        }
    }

    pub(super) fn parse(&self, text: &str) -> Option<PropertyValue> {
        let text = text.trim();
        match self {
            // Parse directly into the declared type to avoid double rounding.
            Self::F32 => {
                let value = text.parse::<f32>().ok()?;
                value.is_finite().then_some(PropertyValue::F32(value))
            }
            Self::I32 | Self::U32 => {
                let value: f64 = text.parse().ok()?;
                (value.fract() == 0.)
                    .then(|| self.value_from_number(value))
                    .flatten()
            }
        }
    }
}
