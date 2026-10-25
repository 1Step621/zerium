use zerium_core::{
    property::{PropertyElementId, PropertySchema, PropertyValue},
    timeline::{
        EffectInstanceId, EvaluatedItem, Frame, ProjectResolution, PropertyAddress, TimelineItem,
        TimelineTime, ValueEdit, ValueEditTarget,
    },
};

use crate::ui::panels::preview::Preview;
use gpui::{
    Bounds, Context, CursorStyle, Hsla, Pixels, Point, div, point, prelude::*, px, relative, size,
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
struct PreviewScalarControl {
    scalar: PreviewScalarValue,
    position: [f32; 2],
    units_per_value: f32,
}

impl PreviewScalarControl {
    fn value_at(&self, delta: [f32; 2]) -> f32 {
        self.scalar.value + delta[self.scalar.axis()] / self.units_per_value
    }

    fn same_target(&self, other: &Self) -> bool {
        self.scalar.address == other.scalar.address && self.scalar.stop == other.scalar.stop
    }

    fn cursor(controls: &[Self]) -> CursorStyle {
        let Some(first) = controls.first() else {
            return CursorStyle::Arrow;
        };
        if controls
            .iter()
            .any(|control| control.scalar.axis() != first.scalar.axis())
        {
            CursorStyle::Crosshair
        } else if first.scalar.axis() == 0 {
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
                let extent = if control.scalar.axis() == 0 {
                    size(px(8.), px(20.))
                } else {
                    size(px(20.), px(8.))
                };
                Bounds::new(
                    position - point(extent.width / 2., extent.height / 2.),
                    extent,
                )
                .contains(&pointer)
                .then(|| {
                    let delta = pointer - position;
                    let distance = f32::from(delta.x).powi(2) + f32::from(delta.y).powi(2);
                    (control.clone(), distance)
                })
            })
            .collect::<Vec<_>>();
        candidates.sort_by(|(left, a), (right, b)| {
            a.total_cmp(b).then(
                left.scalar
                    .time_distance
                    .total_cmp(&right.scalar.time_distance),
            )
        });
        let mut targets = Vec::new();
        for (control, _) in candidates {
            // Each scalar picks one key, including coincident authored keys.
            // Different axes remain independent so an XY handle can move both.
            if !targets
                .iter()
                .any(|target: &Self| control.scalar.address == target.scalar.address)
            {
                targets.push(control);
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
    controls: Vec<PreviewScalarControl>,
    bounds: Option<PreviewBounds>,
    points: Vec<[f32; 2]>,
    motion_paths: Vec<Vec<[f32; 2]>>,
    spline_path: Vec<[f32; 2]>,
}

pub(in crate::ui::panels::preview) struct PreviewEditorDrag {
    controls: Vec<PreviewScalarControl>,
    pointer: Point<Pixels>,
    composition_units_per_pixel: f32,
    pub(in crate::ui::panels::preview) edit: ValueEdit,
}

mod editing;
mod model;
mod render;
