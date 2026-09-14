//! Scalar interpolation and eligibility, independent of editor presentation.
use crate::domain::parameter::ParameterValue;

pub(crate) fn interpolate_scalar(
    from_value: &ParameterValue,
    to_value: &ParameterValue,
    progress: f32,
) -> Option<ParameterValue> {
    if !progress.is_finite() {
        return None;
    }
    let lerp = |from: f32, to: f32| from + (to - from) * progress;
    match (from_value, to_value) {
        (ParameterValue::F32(from), ParameterValue::F32(to)) => {
            Some(ParameterValue::F32(lerp(*from, *to)))
        }
        (ParameterValue::I32(from), ParameterValue::I32(to)) => {
            let progress = f64::from(progress);
            let value = f64::from(*from) + (f64::from(*to) - f64::from(*from)) * progress;
            Some(ParameterValue::I32(
                value
                    .round()
                    .clamp(f64::from(i32::MIN), f64::from(i32::MAX)) as i32,
            ))
        }
        (ParameterValue::U32(from), ParameterValue::U32(to)) => {
            let progress = f64::from(progress);
            let value = f64::from(*from) + (f64::from(*to) - f64::from(*from)) * progress;
            Some(ParameterValue::U32(
                value.round().clamp(0.0, f64::from(u32::MAX)) as u32,
            ))
        }
        (ParameterValue::Color(from), ParameterValue::Color(to)) => Some(ParameterValue::Color([
            lerp(from[0], to[0]),
            lerp(from[1], to[1]),
            lerp(from[2], to[2]),
            lerp(from[3], to[3]),
        ])),
        _ => None,
    }
}
