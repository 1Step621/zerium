//! Logical composition-space bounds, independent of the texture resolution.

use crate::domain::plugin::{OutputBoundsSchema, program_context};
use crate::domain::property::PropertyValues;

use super::RenderSize;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct SurfaceRect {
    pub min: [f64; 2],
    pub max: [f64; 2],
}

impl SurfaceRect {
    pub(crate) fn viewport(size: RenderSize) -> Self {
        let half = [f64::from(size.width) * 0.5, f64::from(size.height) * 0.5];
        Self {
            min: [-half[0], -half[1]],
            max: half,
        }
    }

    pub(crate) fn union(self, other: Self) -> Self {
        Self {
            min: [self.min[0].min(other.min[0]), self.min[1].min(other.min[1])],
            max: [self.max[0].max(other.max[0]), self.max[1].max(other.max[1])],
        }
    }

    pub(crate) fn is_valid(self) -> bool {
        (0..2).all(|axis| {
            self.min[axis].is_finite()
                && self.max[axis].is_finite()
                && self.max[axis] >= self.min[axis]
        })
    }

    /// Aligns a logical rectangle to the output pixel grid at the requested
    /// render scale. The viewport's upper-left corner is pixel (0, 0).
    pub(crate) fn pixel_aligned(
        self,
        viewport: RenderSize,
        composition: RenderSize,
        scale: u32,
    ) -> Option<(Self, RenderSize)> {
        if !self.is_valid()
            || viewport.width == 0
            || viewport.height == 0
            || composition.width == 0
            || composition.height == 0
            || scale == 0
        {
            return None;
        }
        let density = [
            f64::from(viewport.width) * f64::from(scale) / f64::from(composition.width),
            f64::from(viewport.height) * f64::from(scale) / f64::from(composition.height),
        ];
        let origin = [
            f64::from(composition.width) * 0.5,
            f64::from(composition.height) * 0.5,
        ];
        let mut min = [0.0; 2];
        let mut max = [0.0; 2];
        let mut pixels = [0_u32; 2];
        for axis in 0..2 {
            let first = ((self.min[axis] + origin[axis]) * density[axis]).floor();
            let last = ((self.max[axis] + origin[axis]) * density[axis]).ceil();
            if !first.is_finite() || !last.is_finite() {
                return None;
            }
            // An edge-on surface can cover zero whole pixels on one axis.
            // Keep a one-pixel target so its (transparent) shader pass can run.
            let last = last.max(first + 1.0);
            let count = last - first;
            if !count.is_finite() || count < 1.0 || count > f64::from(u32::MAX) {
                return None;
            }
            pixels[axis] = count as u32;
            min[axis] = first / density[axis] - origin[axis];
            max[axis] = last / density[axis] - origin[axis];
        }
        Some((
            Self { min, max },
            RenderSize {
                width: pixels[0],
                height: pixels[1],
            },
        ))
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct BoundsOperation {
    min: [evalexpr::Node; 2],
    max: [evalexpr::Node; 2],
    values: PropertyValues,
}

impl BoundsOperation {
    pub(crate) fn apply(&self, input: SurfaceRect, viewport: SurfaceRect) -> SurfaceRect {
        let mut context = program_context(
            input.min,
            input.max,
            viewport.min,
            viewport.max,
            &self.values,
        );
        let evaluate = |expressions: &[evalexpr::Node; 2],
                        context: &mut evalexpr::HashMapContext| {
            std::array::from_fn(|axis| {
                expressions[axis]
                    .eval_number_with_context_mut(context)
                    .unwrap_or(f64::NAN)
            })
        };
        SurfaceRect {
            min: evaluate(&self.min, &mut context),
            max: evaluate(&self.max, &mut context),
        }
    }

    pub(crate) fn from_schema(schema: &OutputBoundsSchema, values: &PropertyValues) -> Self {
        let OutputBoundsSchema { min, max, .. } = schema;
        Self {
            min: min.clone(),
            max: max.clone(),
            values: values.clone(),
        }
    }
}

pub(super) fn item_bounds(
    schema: &OutputBoundsSchema,
    values: &PropertyValues,
    composition: RenderSize,
) -> SurfaceRect {
    let viewport = SurfaceRect::viewport(composition);
    BoundsOperation::from_schema(schema, values).apply(viewport, viewport)
}
