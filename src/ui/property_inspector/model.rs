use super::control::ArrayGroup;
use super::*;

pub(super) fn append_default(field: &ArrayGroup) -> ParameterValue {
    let ui = field.parameter.ui();
    let constraints = field.parameter.constraints();
    let display_scale = f64::from(ui.display_scale());
    let step = f64::from(ui.step()) * display_scale;
    let min = PropertyInspector::scaled_display_value(
        constraints.min.unwrap_or(f64::from(f32::MIN)),
        display_scale,
    );
    let max = PropertyInspector::scaled_display_value(
        constraints.max.unwrap_or(f64::from(f32::MAX)),
        display_scale,
    );
    let clamp =
        |value: f64| snap_to_step(value * display_scale, step).clamp(min, max) / display_scale;
    let value = match field.parameter.ty().element_type() {
        ParameterValueType::Scalar(ScalarParameterType::F32) => {
            ParameterValue::F32(clamp(0.) as f32)
        }
        ParameterValueType::Scalar(ScalarParameterType::I32) => {
            ParameterValue::I32(clamp(0.).round() as i32)
        }
        ParameterValueType::Scalar(ScalarParameterType::U32) => {
            ParameterValue::U32(clamp(0.).round().max(0.) as u32)
        }
        ParameterValueType::Scalar(ScalarParameterType::Color) => {
            interpolated_or_last(field).unwrap_or(ParameterValue::Color([0., 0., 0., 1.]))
        }
        ParameterValueType::Tuple(tuple) => interpolated_or_last(field).unwrap_or_else(|| {
            ParameterValue::Tuple(
                tuple
                    .elements()
                    .iter()
                    .map(|element| match element {
                        ScalarParameterType::F32 => ParameterValue::F32(0.),
                        ScalarParameterType::I32 => ParameterValue::I32(0),
                        ScalarParameterType::U32 => ParameterValue::U32(0),
                        ScalarParameterType::Bool => ParameterValue::Bool(false),
                        ScalarParameterType::Color => ParameterValue::Color([0., 0., 0., 1.]),
                        ScalarParameterType::String => ParameterValue::String(String::new()),
                        ScalarParameterType::Enum(ty) => ParameterValue::Enum(ty.values()[0]),
                    })
                    .collect(),
            )
        }),
        ParameterValueType::Scalar(ScalarParameterType::Enum(ty)) => {
            ParameterValue::Enum(ty.values()[0])
        }
        ParameterValueType::Scalar(ScalarParameterType::Bool) => ParameterValue::Bool(false),
        ParameterValueType::Scalar(ScalarParameterType::String) => {
            ParameterValue::String(String::new())
        }
    };
    field
        .parameter
        .constraints()
        .clamp_value(&value)
        .unwrap_or(value)
}

fn interpolated_or_last(field: &ArrayGroup) -> Option<ParameterValue> {
    match field.values.as_slice() {
        [first, .., last] => array_element_midpoint(first, last).or_else(|| Some(last.clone())),
        [.., last] => Some(last.clone()),
        [] => None,
    }
}

// Array insertion chooses a midpoint for numeric tuples and colors. This
// container policy is independent of the scalar-only animation tracks.
fn array_element_midpoint(first: &ParameterValue, last: &ParameterValue) -> Option<ParameterValue> {
    use crate::domain::animation::interpolate_scalar;
    match (first, last) {
        (ParameterValue::Tuple(first), ParameterValue::Tuple(last))
            if first.len() == last.len()
                && first
                    .iter()
                    .chain(last)
                    .all(|value| value.numeric_scalar().is_some()) =>
        {
            first
                .iter()
                .zip(last)
                .map(|(first, last)| interpolate_scalar(first, last, 0.5))
                .collect::<Option<Vec<_>>>()
                .map(ParameterValue::Tuple)
        }
        (ParameterValue::Color(_), ParameterValue::Color(_)) => {
            interpolate_scalar(first, last, 0.5)
        }
        _ => None,
    }
}

pub(super) fn snap_to_step(value: f64, step: f64) -> f64 {
    if !step.is_finite() || step <= 0. {
        value
    } else {
        (value / step).round() * step
    }
}
