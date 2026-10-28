use zerium_core::{
    property::{PropertyElementId, PropertySchema, PropertyValue},
    timeline::{
        EffectInstanceId, EvaluatedItem, Frame, ProjectResolution, PropertyAddress, TimelineItem,
        TimelineTime, ValueEdit, ValueEditTarget,
    },
};

use crate::ui::panels::preview::Preview;
use gpui::{
    Bounds, Context, CursorStyle, Hsla, Pixels, Point, Size, div, point, prelude::*, px, relative,
    size,
};

#[derive(Clone)]
struct PreviewScalarValue {
    address: PropertyAddress,
    value: f32,
    stop: Option<usize>,
    time_distance: f64,
}

impl PreviewScalarValue {
    fn axis(&self) -> usize {
        self.address
            .scalar_index
            .expect("preview scalar controls address one axis")
    }
}

#[derive(Clone)]
struct PreviewControl {
    position: [f32; 2],
    kind: PreviewControlKind,
}

#[derive(Clone)]
enum PreviewControlKind {
    Scalar {
        scalar: PreviewScalarValue,
        units_per_value: f32,
    },
    SizeCorner {
        scalars: [PreviewScalarValue; 2],
        corner: [u8; 2],
        scale: PreviewSizeScale,
        time_distance: f64,
    },
}

#[derive(Clone, Copy)]
struct PreviewSizeScale {
    direction: [f64; 2],
    min: f64,
    max: f64,
}

impl PreviewSizeScale {
    fn new(
        values: [f32; 2],
        corner: [u8; 2],
        origin: [f32; 2],
        bounds: [(f64, f64); 2],
    ) -> Option<Self> {
        if !values.iter().all(|value| value.is_finite() && *value > 0.) {
            return None;
        }
        let direction = [0, 1].map(|axis| {
            (f64::from(corner[axis]) - f64::from(origin[axis])) * f64::from(values[axis])
        });
        if direction == [0., 0.] {
            return None;
        }
        let mut min = 0_f64;
        let mut max = f64::INFINITY;
        for (value, (low, high)) in values.into_iter().zip(bounds) {
            min = min.max(low / f64::from(value));
            max = max.min(high / f64::from(value));
        }
        (min <= max).then_some(Self {
            direction,
            min,
            max,
        })
    }

    fn factor(self, delta: [f32; 2]) -> f64 {
        // Project the pointer onto the corner's ray from the fixed origin.
        let distance = self
            .direction
            .into_iter()
            .map(|value| value * value)
            .sum::<f64>();
        let movement = self
            .direction
            .into_iter()
            .zip(delta)
            .map(|(direction, delta)| direction * f64::from(delta))
            .sum::<f64>();
        (1. + movement / distance).clamp(self.min, self.max)
    }
}

impl PreviewControl {
    fn scalars(&self) -> &[PreviewScalarValue] {
        match &self.kind {
            PreviewControlKind::Scalar { scalar, .. } => std::slice::from_ref(scalar),
            PreviewControlKind::SizeCorner { scalars, .. } => scalars,
        }
    }

    fn values_at(&self, delta: [f32; 2]) -> impl Iterator<Item = f32> {
        let (first, second) = match &self.kind {
            PreviewControlKind::Scalar {
                scalar,
                units_per_value,
            } => (scalar.value + delta[scalar.axis()] / units_per_value, None),
            PreviewControlKind::SizeCorner { scalars, scale, .. } => {
                let factor = scale.factor(delta);
                let [width, height] = scalars
                    .each_ref()
                    .map(|scalar| (f64::from(scalar.value) * factor) as f32);
                (width, Some(height))
            }
        };
        std::iter::once(first).chain(second)
    }

    fn axis(&self) -> Option<usize> {
        match &self.kind {
            PreviewControlKind::Scalar { scalar, .. } => Some(scalar.axis()),
            PreviewControlKind::SizeCorner { .. } => None,
        }
    }

    fn corner(&self) -> Option<[u8; 2]> {
        match self.kind {
            PreviewControlKind::Scalar { .. } => None,
            PreviewControlKind::SizeCorner { corner, .. } => Some(corner),
        }
    }

    fn time_distance(&self) -> f64 {
        match &self.kind {
            PreviewControlKind::Scalar { scalar, .. } => scalar.time_distance,
            PreviewControlKind::SizeCorner { time_distance, .. } => *time_distance,
        }
    }

    fn same_target(&self, other: &Self) -> bool {
        self.corner() == other.corner()
            && self.scalars().len() == other.scalars().len()
            && self
                .scalars()
                .iter()
                .zip(other.scalars())
                .all(|(a, b)| a.address == b.address && a.stop == b.stop)
    }

    fn handle_size(&self) -> Size<Pixels> {
        match self.axis() {
            Some(0) => size(px(2.), px(12.)),
            Some(_) => size(px(12.), px(2.)),
            None => size(px(6.), px(6.)),
        }
    }

    fn hit_size(&self) -> Size<Pixels> {
        match self.axis() {
            Some(0) => size(px(8.), px(20.)),
            Some(_) => size(px(20.), px(8.)),
            None => size(px(14.), px(14.)),
        }
    }

    fn cursor(controls: &[Self]) -> CursorStyle {
        let Some(first) = controls.first() else {
            return CursorStyle::Arrow;
        };
        if let Some(corner) = first.corner() {
            return if corner[0] == corner[1] {
                CursorStyle::ResizeUpLeftDownRight
            } else {
                CursorStyle::ResizeUpRightDownLeft
            };
        }
        if controls
            .iter()
            .any(|control| control.axis() != first.axis())
        {
            CursorStyle::Crosshair
        } else if first.axis() == Some(0) {
            CursorStyle::ResizeLeftRight
        } else {
            CursorStyle::ResizeUpDown
        }
    }

    fn screen_position(
        &self,
        bounds: Bounds<Pixels>,
        resolution: ProjectResolution,
    ) -> Point<Pixels> {
        point(
            bounds.origin.x
                + bounds.size.width * (self.position[0] / resolution.width() as f32 + 0.5),
            bounds.origin.y
                + bounds.size.height * (self.position[1] / resolution.height() as f32 + 0.5),
        )
    }

    fn at_pointer(
        controls: &[Self],
        pointer: Point<Pixels>,
        bounds: Bounds<Pixels>,
        resolution: ProjectResolution,
    ) -> Vec<Self> {
        if !bounds.contains(&pointer) {
            return Vec::new();
        }
        let mut candidates = controls
            .iter()
            .filter_map(|control| {
                let position = control.screen_position(bounds, resolution);
                let extent = control.hit_size();
                Bounds::new(
                    position - point(extent.width / 2., extent.height / 2.),
                    extent,
                )
                .contains(&pointer)
                .then(|| {
                    let delta = pointer - position;
                    let distance = f32::from(delta.x).powi(2) + f32::from(delta.y).powi(2);
                    (control, distance)
                })
            })
            .collect::<Vec<_>>();
        candidates.sort_by(|(left, a), (right, b)| {
            a.total_cmp(b)
                .then(left.time_distance().total_cmp(&right.time_distance()))
        });
        let Some((first, _)) = candidates.first() else {
            return Vec::new();
        };
        if first.corner().is_some() {
            return vec![(*first).clone()];
        }
        let mut targets = Vec::new();
        for (control, _) in candidates {
            if control.corner().is_some() {
                continue;
            }
            // Each scalar picks one key, including coincident authored keys.
            // Different axes remain independent so an XY handle can move both.
            if !targets
                .iter()
                .any(|target: &Self| control.scalars()[0].address == target.scalars()[0].address)
            {
                targets.push(control.clone());
            }
        }
        targets
    }
}

#[derive(Clone, Copy)]
struct PreviewBounds {
    center: [f32; 2],
    size: [f32; 2],
}

pub(in crate::ui::panels::preview) struct PreviewEditorOverlay {
    controls: Vec<PreviewControl>,
    bounds: Option<PreviewBounds>,
    points: Vec<[f32; 2]>,
    motion_paths: Vec<Vec<[f32; 2]>>,
    spline_path: Vec<[f32; 2]>,
}

pub(in crate::ui::panels::preview) struct PreviewEditorDrag {
    controls: Vec<PreviewControl>,
    pointer: Point<Pixels>,
    composition_units_per_pixel: f32,
    pub(in crate::ui::panels::preview) edit: ValueEdit,
}

mod editing;
mod model;
mod render;
