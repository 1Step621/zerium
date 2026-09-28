//! Logical composition-space bounds, independent of the texture resolution.

use crate::domain::plugin::{ItemBoundsSchema, OutputBoundsSchema};
use crate::domain::property::{PropertyValue, PropertyValues};

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

    pub(crate) fn quad(center: [f64; 2], size: [f64; 2], degrees: f64) -> Self {
        let half = [size[0].abs() * 0.5, size[1].abs() * 0.5];
        let (sine, cosine) = degrees.to_radians().sin_cos();
        let extent = [
            cosine.abs() * half[0] + sine.abs() * half[1],
            sine.abs() * half[0] + cosine.abs() * half[1],
        ];
        Self {
            min: [center[0] - extent[0], center[1] - extent[1]],
            max: [center[0] + extent[0], center[1] + extent[1]],
        }
    }

    pub(crate) fn translate(self, offset: [f64; 2]) -> Self {
        Self {
            min: [self.min[0] + offset[0], self.min[1] + offset[1]],
            max: [self.max[0] + offset[0], self.max[1] + offset[1]],
        }
    }

    pub(crate) fn outset(self, radius: f64) -> Self {
        let radius = radius.max(0.0);
        Self {
            min: [self.min[0] - radius, self.min[1] - radius],
            max: [self.max[0] + radius, self.max[1] + radius],
        }
    }

    pub(crate) fn union(self, other: Self) -> Self {
        Self {
            min: [self.min[0].min(other.min[0]), self.min[1].min(other.min[1])],
            max: [self.max[0].max(other.max[0]), self.max[1].max(other.max[1])],
        }
    }

    pub(crate) fn rotate(self, center: [f64; 2], degrees: f64) -> Self {
        let (sine, cosine) = degrees.to_radians().sin_cos();
        let mut min = [f64::INFINITY; 2];
        let mut max = [f64::NEG_INFINITY; 2];
        for x in [self.min[0], self.max[0]] {
            for y in [self.min[1], self.max[1]] {
                let p = [x - center[0], y - center[1]];
                let rotated = [
                    center[0] + p[0] * cosine - p[1] * sine,
                    center[1] + p[0] * sine + p[1] * cosine,
                ];
                for axis in 0..2 {
                    min[axis] = min[axis].min(rotated[axis]);
                    max[axis] = max[axis].max(rotated[axis]);
                }
            }
        }
        Self { min, max }
    }

    pub(crate) fn perspective(
        self,
        center: [f64; 2],
        rotation: [f64; 3],
        focal_length: f64,
    ) -> Self {
        let radians = rotation.map(f64::to_radians);
        let sine = radians.map(f64::sin);
        let cosine = radians.map(f64::cos);
        let r00 = cosine[2] * cosine[1];
        let r10 = sine[2] * cosine[1];
        let r20 = -sine[1];
        let r01 = cosine[2] * sine[1] * sine[0] - sine[2] * cosine[0];
        let r11 = sine[2] * sine[1] * sine[0] + cosine[2] * cosine[0];
        let r21 = cosine[1] * sine[0];
        let focal_length = focal_length.max(1.0);
        let mut min = [f64::INFINITY; 2];
        let mut max = [f64::NEG_INFINITY; 2];
        for x in [self.min[0], self.max[0]] {
            for y in [self.min[1], self.max[1]] {
                let source = [x - center[0], y - center[1]];
                let depth = focal_length + r20 * source[0] + r21 * source[1];
                if depth <= 0.00001 {
                    return Self {
                        min: [f64::NAN; 2],
                        max: [f64::NAN; 2],
                    };
                }
                let projected = [
                    center[0] + focal_length * (r00 * source[0] + r01 * source[1]) / depth,
                    center[1] + focal_length * (r10 * source[0] + r11 * source[1]) / depth,
                ];
                for axis in 0..2 {
                    min[axis] = min[axis].min(projected[axis]);
                    max[axis] = max[axis].max(projected[axis]);
                }
            }
        }
        Self { min, max }
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

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum BoundsOperation {
    Same,
    Viewport,
    Translate([f64; 2]),
    CenterRange {
        range: SurfaceRect,
        padding: f64,
    },
    Outset(f64),
    Rotate {
        center: [f64; 2],
        degrees: f64,
    },
    Perspective {
        center: [f64; 2],
        rotation: [f64; 3],
        focal_length: f64,
    },
}

impl BoundsOperation {
    pub(crate) fn apply(self, input: SurfaceRect, viewport: SurfaceRect) -> SurfaceRect {
        match self {
            Self::Same => input,
            Self::Viewport => viewport,
            Self::Translate(offset) => input.translate(offset),
            Self::CenterRange { range, padding } => {
                let half = [
                    (input.max[0] - input.min[0]) * 0.5,
                    (input.max[1] - input.min[1]) * 0.5,
                ];
                SurfaceRect {
                    min: [range.min[0] - half[0], range.min[1] - half[1]],
                    max: [range.max[0] + half[0], range.max[1] + half[1]],
                }
                .outset(padding)
            }
            Self::Outset(radius) => input.outset(radius),
            Self::Rotate { center, degrees } => input.rotate(center, degrees),
            Self::Perspective {
                center,
                rotation,
                focal_length,
            } => input.perspective(center, rotation, focal_length),
        }
    }

    pub(crate) fn from_schema(schema: &OutputBoundsSchema, values: &PropertyValues) -> Self {
        match schema {
            OutputBoundsSchema::Same => Self::Same,
            OutputBoundsSchema::Viewport => Self::Viewport,
            OutputBoundsSchema::Translate { offset } => Self::Translate(pair(values, offset)),
            OutputBoundsSchema::Outset { radius, multiplier } => {
                Self::Outset(scalar(values, radius).abs() * f64::from(*multiplier))
            }
            OutputBoundsSchema::Rotate { angle, center } => Self::Rotate {
                center: pair(values, center),
                degrees: scalar(values, angle),
            },
            OutputBoundsSchema::Perspective {
                rotation,
                center,
                perspective,
            } => Self::Perspective {
                center: pair(values, center),
                rotation: triple(values, rotation),
                focal_length: scalar(values, perspective),
            },
            OutputBoundsSchema::CenterRange {
                position,
                size,
                size_outset,
                padding,
            } => Self::CenterRange {
                range: SurfaceRect::quad(
                    pair(values, position),
                    outset_size(pair(values, size), *size_outset),
                    0.0,
                ),
                padding: f64::from(*padding),
            },
        }
    }
}

pub(super) fn item_bounds(
    schema: &ItemBoundsSchema,
    values: &PropertyValues,
    composition: RenderSize,
) -> SurfaceRect {
    match schema {
        ItemBoundsSchema::Viewport => SurfaceRect::viewport(composition),
        ItemBoundsSchema::Quad {
            position,
            size,
            size_outset,
            rotation,
            padding,
        } => SurfaceRect::quad(
            pair(values, position),
            outset_size(pair(values, size), *size_outset),
            rotation.as_ref().map_or(0.0, |id| scalar(values, id)),
        )
        .outset(padding.as_ref().map_or(0.0, |id| scalar(values, id).abs())),
    }
}

fn outset_size(size: [f64; 2], fraction: f32) -> [f64; 2] {
    size.map(|component| component * (1.0 + 2.0 * f64::from(fraction)))
}

fn scalar(values: &PropertyValues, id: &str) -> f64 {
    match values.property(id) {
        Some(PropertyValue::F32(value)) => f64::from(*value),
        _ => 0.0,
    }
}

fn pair(values: &PropertyValues, id: &str) -> [f64; 2] {
    match values.property(id) {
        Some(PropertyValue::Tuple(values)) => match values.as_slice() {
            [PropertyValue::F32(x), PropertyValue::F32(y)] => [f64::from(*x), f64::from(*y)],
            _ => [0.0; 2],
        },
        _ => [0.0; 2],
    }
}

fn triple(values: &PropertyValues, id: &str) -> [f64; 3] {
    match values.property(id) {
        Some(PropertyValue::Tuple(values)) => match values.as_slice() {
            [
                PropertyValue::F32(x),
                PropertyValue::F32(y),
                PropertyValue::F32(z),
            ] => [f64::from(*x), f64::from(*y), f64::from(*z)],
            _ => [0.0; 3],
        },
        _ => [0.0; 3],
    }
}
