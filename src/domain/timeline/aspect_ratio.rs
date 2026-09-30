use serde::{Deserialize, Serialize};

use crate::domain::property::{PropertySchema, PropertyValue};

/// A retained direction for a pair. `None` on the owner means unlocked.
/// Keeping the direction separately allows a collapsed pair to grow again.
#[derive(Clone, Copy, Debug, PartialEq, Deserialize, Serialize)]
#[serde(try_from = "[f64; 2]", into = "[f64; 2]")]
pub(crate) struct AspectRatio([f64; 2]);

impl TryFrom<[f64; 2]> for AspectRatio {
    type Error = &'static str;

    fn try_from(pair: [f64; 2]) -> Result<Self, Self::Error> {
        if !pair.iter().all(|value| value.is_finite()) {
            return Err("aspect ratio must be finite");
        }
        let magnitude = pair[0].abs().max(pair[1].abs());
        Ok(Self(if magnitude == 0. {
            [1., 1.]
        } else {
            pair.map(|value| value / magnitude)
        }))
    }
}

impl From<AspectRatio> for [f64; 2] {
    fn from(ratio: AspectRatio) -> Self {
        ratio.0
    }
}

impl AspectRatio {
    pub(crate) fn from_value(value: &PropertyValue) -> Option<Self> {
        Self::try_from([
            value.scalar_at(Some(0))?.numeric_scalar()?,
            value.scalar_at(Some(1))?.numeric_scalar()?,
        ])
        .ok()
    }

    /// Projects a direct edit onto the retained ratio, intersecting both axes'
    /// constraints before writing either component. Whole-pair edits use X.
    pub(super) fn constrain(
        self,
        property: &PropertySchema,
        value: &PropertyValue,
        edited_axis: Option<usize>,
    ) -> Option<PropertyValue> {
        if !property.ty().allows(value) {
            return None;
        }
        let mut axis = edited_axis.unwrap_or(0);
        if axis > 1 {
            return None;
        }
        let requested = [
            value.scalar_at(Some(0))?.numeric_scalar()?,
            value.scalar_at(Some(1))?.numeric_scalar()?,
        ];
        if self.0[axis] == 0. {
            if requested[axis] != 0. {
                return None;
            }
            axis = 1 - axis;
        }
        let mut min = f64::NEG_INFINITY;
        let mut max = f64::INFINITY;
        for (index, direction) in self.0.into_iter().enumerate() {
            let bounds = property.configuration_constraints(Some(index));
            let low = bounds
                .min
                .unwrap_or(f64::from(f32::MIN))
                .max(f64::from(f32::MIN));
            let high = bounds
                .max
                .unwrap_or(f64::from(f32::MAX))
                .min(f64::from(f32::MAX));
            if direction == 0. {
                if !(low..=high).contains(&0.) {
                    return None;
                }
            } else {
                let (a, b) = (low / direction, high / direction);
                min = min.max(a.min(b));
                max = max.min(a.max(b));
            }
        }
        if min > max {
            return None;
        }
        let scale = (requested[axis] / self.0[axis]).clamp(min, max);
        let result = PropertyValue::f32_tuple(self.0.map(|direction| (direction * scale) as f32));
        property.accepts_value(&result).then_some(result)
    }
}
