use zerium_core::{
    property::{PropertyElementId, PropertySchema, PropertyValue},
    timeline::{
        EditGesture, EffectInstanceId, Frame, PropertyAddress, TimelineEditor, TimelineItem,
        TimelineTime,
    },
};

use super::Preview;
use gpui::{
    Bounds, Context, CursorStyle, Hsla, Pixels, Point, div, point, prelude::*, px, relative, size,
};
use zerium_core::timeline::ProjectResolution;

#[derive(Clone)]
struct PreviewScalarValue {
    address: PropertyAddress,
    value: f32,
    stop: Option<usize>,
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
        candidates.sort_by(|(_, a), (_, b)| a.total_cmp(b));
        let mut targets = Vec::new();
        for (control, _) in candidates {
            // Opposite size handles can overlap but still edit the same value.
            if !targets.iter().any(|target| control.same_target(target)) {
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

pub(super) struct PreviewEditorOverlay {
    controls: Vec<PreviewScalarControl>,
    bounds: Option<PreviewBounds>,
    points: Vec<[f32; 2]>,
    motion_paths: Vec<Vec<[f32; 2]>>,
    spline_path: Vec<[f32; 2]>,
}

pub(super) struct PreviewEditorDrag {
    controls: Vec<PreviewScalarControl>,
    pointer: Point<Pixels>,
    composition_units_per_pixel: f32,
    pub(super) gesture: EditGesture,
}

impl PreviewEditorDrag {
    fn value_at(&self, control: &PreviewScalarControl, pointer: Point<Pixels>) -> f32 {
        let delta = pointer - self.pointer;
        let delta = if control.scalar.axis() == 0 {
            delta.x
        } else {
            delta.y
        };
        control.scalar.value
            + f32::from(delta) * self.composition_units_per_pixel / control.units_per_value
    }
}

impl Preview {
    fn f32_pair(value: &PropertyValue) -> Option<[f32; 2]> {
        Some([
            value.scalar_at(Some(0))?.numeric_scalar()? as f32,
            value.scalar_at(Some(1))?.numeric_scalar()? as f32,
        ])
    }

    fn item_position(
        item: &TimelineItem,
        effect_id: Option<EffectInstanceId>,
        property_id: &str,
    ) -> [f32; 2] {
        item.property_values(effect_id)
            .and_then(|properties| properties.property(property_id))
            .and_then(Self::f32_pair)
            .filter(|value| value.iter().all(|value| value.is_finite()))
            .unwrap_or([0., 0.])
    }

    fn item_origin(
        item: &TimelineItem,
        effect_id: Option<EffectInstanceId>,
        property_id: Option<&str>,
    ) -> [f32; 2] {
        property_id
            .and_then(|id| item.property_values(effect_id)?.property(id))
            .and_then(|value| {
                let PropertyValue::Tuple(values) = value else {
                    return None;
                };
                let [PropertyValue::Enum(x), PropertyValue::Enum(y)] = values.as_slice() else {
                    return None;
                };
                Some([*x as f32 * 0.5, *y as f32 * 0.5])
            })
            .unwrap_or([0.5, 0.5])
    }

    fn item_center(
        item: &TimelineItem,
        effect_id: Option<EffectInstanceId>,
        position: &str,
        size: [f32; 2],
        origin: Option<&str>,
    ) -> [f32; 2] {
        let position = Self::item_position(item, effect_id, position);
        let origin = Self::item_origin(item, effect_id, origin);
        [0, 1].map(|axis| position[axis] + (0.5 - origin[axis]) * size[axis])
    }

    fn position_at_time(
        item: &TimelineItem,
        effect_id: Option<EffectInstanceId>,
        property: &PropertySchema,
        time: TimelineTime,
    ) -> Option<[f32; 2]> {
        Self::f32_pair(&item.evaluated_property_at(time, effect_id, property)?)
            .filter(|value| value.iter().all(|value| value.is_finite()))
    }

    fn item_size(
        item: &TimelineItem,
        effect_id: Option<EffectInstanceId>,
        property_id: &str,
    ) -> Option<[f32; 2]> {
        Self::f32_pair(item.property_values(effect_id)?.property(property_id)?)
            .filter(|value| value.iter().all(|value| value.is_finite() && *value > 0.))
    }
}

mod editing;
mod model;
mod motion_path;
mod render;
mod spline;
