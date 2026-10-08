use zerium_core::{
    property::{PropertyElementId, PropertySchema, PropertyValue},
    timeline::{
        EffectInstanceId, Frame, PropertyAddress, TimelineEditor, TimelineItem, TimelineTime,
    },
};

use super::Preview;
use gpui::{
    Context, CursorStyle, Empty, EntityId, Hsla, MouseButton, MouseDownEvent, Render, SharedString,
    Window, div, prelude::*, px, relative,
};

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
    fn key(&self) -> SharedString {
        format!(
            "preview-scalar-{:?}-{:?}-{}",
            self.scalar.address,
            self.scalar.stop,
            self.units_per_value.is_sign_negative()
        )
        .into()
    }

    fn cursor(&self) -> CursorStyle {
        if self.scalar.axis() == 0 {
            CursorStyle::ResizeLeftRight
        } else {
            CursorStyle::ResizeUpDown
        }
    }
}

#[derive(Clone)]
pub(super) struct PreviewEditorDrag {
    preview_id: EntityId,
    control_key: SharedString,
}

impl Render for PreviewEditorDrag {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        Empty
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

pub(super) struct PreviewScalarDragOrigin {
    control: PreviewScalarControl,
    pointer: f32,
    composition_units_per_pixel: f32,
}

impl PreviewScalarDragOrigin {
    fn value_at(&self, pointer: [f32; 2]) -> f32 {
        self.control.scalar.value
            + (pointer[self.control.scalar.axis()] - self.pointer)
                * self.composition_units_per_pixel
                / self.control.units_per_value
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
