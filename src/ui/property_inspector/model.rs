use super::control::ElementGroup;
use super::*;

pub(super) fn append_default(group: &ElementGroup) -> PropertyValue {
    let ui = group.property.configuration_ui(None);
    let constraints = group.property.configuration_constraints(None);
    let step = f64::from(ui.step());
    let min = constraints.min.unwrap_or(f64::from(f32::MIN));
    let max = constraints.max.unwrap_or(f64::from(f32::MAX));
    let clamp = |value: f64| snap_to_step(value, step).clamp(min, max);
    let value_type = match group.property.ty() {
        PropertyType::Value(value_type)
        | PropertyType::Array {
            element_type: value_type,
            ..
        } => value_type,
    };
    let value = match value_type {
        PropertyValueType::Scalar(ScalarPropertyType::F32) => PropertyValue::F32(clamp(0.) as f32),
        PropertyValueType::Scalar(ScalarPropertyType::I32) => {
            PropertyValue::I32(clamp(0.).round() as i32)
        }
        PropertyValueType::Scalar(ScalarPropertyType::U32) => {
            PropertyValue::U32(clamp(0.).round().max(0.) as u32)
        }
        PropertyValueType::Scalar(ScalarPropertyType::Color) => {
            interpolated_or_last(group).unwrap_or(PropertyValue::Color([0., 0., 0., 1.]))
        }
        PropertyValueType::Tuple(tuple) => interpolated_or_last(group).unwrap_or_else(|| {
            PropertyValue::Tuple(
                tuple
                    .scalars()
                    .iter()
                    .map(|scalar_type| match scalar_type {
                        ScalarPropertyType::F32 => PropertyValue::F32(0.),
                        ScalarPropertyType::I32 => PropertyValue::I32(0),
                        ScalarPropertyType::U32 => PropertyValue::U32(0),
                        ScalarPropertyType::Bool => PropertyValue::Bool(false),
                        ScalarPropertyType::Color => PropertyValue::Color([0., 0., 0., 1.]),
                        ScalarPropertyType::String => PropertyValue::String(String::new()),
                        ScalarPropertyType::Enum(ty) => PropertyValue::Enum(ty.values()[0]),
                    })
                    .collect(),
            )
        }),
        PropertyValueType::Scalar(ScalarPropertyType::Enum(ty)) => {
            PropertyValue::Enum(ty.values()[0])
        }
        PropertyValueType::Scalar(ScalarPropertyType::Bool) => PropertyValue::Bool(false),
        PropertyValueType::Scalar(ScalarPropertyType::String) => {
            PropertyValue::String(String::new())
        }
    };
    group.property.constrained_value(&value).unwrap_or(value)
}

fn interpolated_or_last(group: &ElementGroup) -> Option<PropertyValue> {
    match group.elements.as_slice() {
        [first, .., last] => {
            element_midpoint(first.value(), last.value()).or_else(|| Some(last.value().clone()))
        }
        [.., last] => Some(last.value().clone()),
        [] => None,
    }
}

// Array insertion chooses a midpoint for numeric tuples and colors. This
// container policy is independent of the scalar-only animation tracks.
fn element_midpoint(first: &PropertyValue, last: &PropertyValue) -> Option<PropertyValue> {
    use crate::domain::animation::interpolate_scalar;
    match (first, last) {
        (PropertyValue::Tuple(first), PropertyValue::Tuple(last))
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
                .map(PropertyValue::Tuple)
        }
        (PropertyValue::Color(_), PropertyValue::Color(_)) => interpolate_scalar(first, last, 0.5),
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
