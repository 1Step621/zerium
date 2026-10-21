//! Logical composition-space bounds, independent of the texture resolution.

use std::collections::BTreeMap;
use zerium_core::plugin::{OutputBoundsSchema, program_context};
use zerium_core::property::PropertyValues;

use super::{RenderSize, RenderView};

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct SurfaceRect {
    pub min: [f64; 2],
    pub max: [f64; 2],
}

impl SurfaceRect {
    pub(crate) const EMPTY: Self = Self {
        min: [0.; 2],
        max: [0.; 2],
    };

    pub(crate) fn is_empty(self) -> bool {
        (0..2).any(|axis| self.max[axis] <= self.min[axis])
    }

    pub(crate) fn intersection(self, other: Self) -> Option<Self> {
        let result = Self {
            min: [self.min[0].max(other.min[0]), self.min[1].max(other.min[1])],
            max: [self.max[0].min(other.max[0]), self.max[1].min(other.max[1])],
        };
        (!result.is_empty()).then_some(result)
    }

    fn expanded(self, padding: [f64; 2]) -> Self {
        Self {
            min: [self.min[0] - padding[0], self.min[1] - padding[1]],
            max: [self.max[0] + padding[0], self.max[1] + padding[1]],
        }
    }

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

    /// Raster allocation is independent of the node's logical content bounds.
    pub(crate) fn pixel_aligned(self, grid: RasterGrid) -> Option<(Self, RenderSize)> {
        if !self.is_valid() || !grid.is_valid() {
            return None;
        }

        let mut min = [0.; 2];
        let mut max = [0.; 2];
        let mut pixels = [0_u32; 2];
        for axis in 0..2 {
            let first = ((self.min[axis] - grid.origin[axis]) * grid.density[axis]).floor();
            let last = ((self.max[axis] - grid.origin[axis]) * grid.density[axis]).ceil();
            let count = (last - first).max(1.);
            if !first.is_finite() || !count.is_finite() || count > f64::from(u32::MAX) {
                return None;
            }

            pixels[axis] = count as u32;
            min[axis] = first / grid.density[axis] + grid.origin[axis];
            max[axis] = (first + count) / grid.density[axis] + grid.origin[axis];
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

/// Sampling density and alignment, carried from a consumer into its inputs.
/// Camera zoom changes the density, never the logical geometry or effect basis.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct RasterGrid {
    origin: [f64; 2],
    density: [f64; 2],
}

impl RasterGrid {
    pub(crate) fn key(self) -> [u64; 4] {
        [
            self.origin[0].to_bits(),
            self.origin[1].to_bits(),
            self.density[0].to_bits(),
            self.density[1].to_bits(),
        ]
    }

    pub(crate) fn new(output: RenderSize, composition: RenderSize, scale: u32) -> Self {
        Self {
            origin: SurfaceRect::viewport(composition).min,
            density: [
                f64::from(output.width) * f64::from(scale) / f64::from(composition.width),
                f64::from(output.height) * f64::from(scale) / f64::from(composition.height),
            ],
        }
    }

    pub(crate) fn is_valid(self) -> bool {
        self.origin.iter().all(|value| value.is_finite())
            && self
                .density
                .iter()
                .all(|value| value.is_finite() && *value > 0.)
    }

    pub(crate) fn through_view(self, view: RenderView) -> Self {
        let (sin, cos) = view.angle.to_radians().sin_cos();
        let (sin, cos, zoom) = (f64::from(sin), f64::from(cos), f64::from(view.zoom));
        Self {
            origin: self.origin,
            density: [
                (cos * self.density[0]).hypot(sin * self.density[1]) * zoom,
                (sin * self.density[0]).hypot(cos * self.density[1]) * zoom,
            ],
        }
    }

    /// Include neighboring source texels for bilinear filtering at crop edges.
    pub(crate) fn sampling_bounds(self, bounds: SurfaceRect) -> SurfaceRect {
        bounds.expanded([1. / self.density[0], 1. / self.density[1]])
    }
}

impl RenderView {
    pub(crate) fn inverse_transform(self) -> [[f64; 3]; 2] {
        let (sin, cos) = self.angle.to_radians().sin_cos();
        let (sin, cos, zoom) = (f64::from(sin), f64::from(cos), f64::from(self.zoom));
        [
            [cos / zoom, -sin / zoom, f64::from(self.position[0])],
            [sin / zoom, cos / zoom, f64::from(self.position[1])],
        ]
    }

    /// Inverse view transform of an output rectangle, in composition coordinates.
    pub(crate) fn source_bounds(self, output: SurfaceRect) -> SurfaceRect {
        let inverse = self.inverse_transform();
        let mut bounds = SurfaceRect {
            min: [f64::INFINITY; 2],
            max: [f64::NEG_INFINITY; 2],
        };

        for x in [output.min[0], output.max[0]] {
            for y in [output.min[1], output.max[1]] {
                for (axis, row) in inverse.iter().enumerate() {
                    let point = row[0] * x + row[1] * y + row[2];
                    bounds.min[axis] = bounds.min[axis].min(point);
                    bounds.max[axis] = bounds.max[axis].max(point);
                }
            }
        }

        bounds
    }
}

/// Logical rectangles available to output-bounds programs.
pub(crate) struct BoundsContext<'a> {
    pub input: SurfaceRect,
    pub source: SurfaceRect,
    pub viewport: SurfaceRect,
    pub images: &'a [(String, SurfaceRect)],
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct BoundsOperation {
    schema: OutputBoundsSchema,
    values: PropertyValues,
    overrides: BTreeMap<String, f64>,
}

impl BoundsOperation {
    pub(crate) fn evaluate(&self, rects: &BoundsContext<'_>) -> Result<SurfaceRect, String> {
        let Self {
            schema,
            values,
            overrides,
        } = self;
        let mut rectangles = vec![
            ("input".to_owned(), rects.input.min, rects.input.max),
            ("source".to_owned(), rects.source.min, rects.source.max),
            (
                "viewport".to_owned(),
                rects.viewport.min,
                rects.viewport.max,
            ),
        ];
        rectangles.extend(
            rects
                .images
                .iter()
                .map(|(id, bounds)| (format!("images::{id}"), bounds.min, bounds.max)),
        );
        let mut context = program_context(
            rectangles
                .iter()
                .map(|(name, min, max)| (name.as_str(), *min, *max)),
            values,
            overrides,
        );
        let [min, max] = schema.evaluate(&mut context)?;
        Ok(SurfaceRect { min, max })
    }

    pub(crate) fn from_schema(
        schema: &OutputBoundsSchema,
        values: &PropertyValues,
        overrides: &BTreeMap<String, f64>,
    ) -> Box<Self> {
        Box::new(Self {
            schema: schema.clone(),
            values: values.clone(),
            overrides: overrides.clone(),
        })
    }
}
