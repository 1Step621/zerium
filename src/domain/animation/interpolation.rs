//! Scalar interpolation and eligibility, independent of editor presentation.
use crate::domain::property::PropertyValue;

pub(crate) fn interpolate_scalar(
    from_value: &PropertyValue,
    to_value: &PropertyValue,
    progress: f32,
) -> Option<PropertyValue> {
    if !progress.is_finite() {
        return None;
    }
    let lerp = |from: f32, to: f32| from + (to - from) * progress;
    match (from_value, to_value) {
        (PropertyValue::F32(from), PropertyValue::F32(to)) => {
            Some(PropertyValue::F32(lerp(*from, *to)))
        }
        (PropertyValue::I32(from), PropertyValue::I32(to)) => {
            let progress = f64::from(progress);
            let value = f64::from(*from) + (f64::from(*to) - f64::from(*from)) * progress;
            Some(PropertyValue::I32(
                value
                    .round()
                    .clamp(f64::from(i32::MIN), f64::from(i32::MAX)) as i32,
            ))
        }
        (PropertyValue::U32(from), PropertyValue::U32(to)) => {
            let progress = f64::from(progress);
            let value = f64::from(*from) + (f64::from(*to) - f64::from(*from)) * progress;
            Some(PropertyValue::U32(
                value.round().clamp(0.0, f64::from(u32::MAX)) as u32,
            ))
        }
        (PropertyValue::Color(from), PropertyValue::Color(to)) => Some(PropertyValue::Color([
            lerp(from[0], to[0]),
            lerp(from[1], to[1]),
            lerp(from[2], to[2]),
            lerp(from[3], to[3]),
        ])),
        _ => None,
    }
}
