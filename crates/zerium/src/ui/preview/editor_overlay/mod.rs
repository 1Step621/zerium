use gpui::{CursorStyle, Empty, EntityId};

use zerium_core::{
    property::{PropertyElementId, PropertyPath, PropertyValue, PropertyValues},
    timeline::{EffectInstanceId, PropertyAddress, TimelineItem, TimelineTime},
};

use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PreviewResizeHandle {
    Left,
    Right,
    Top,
    Bottom,
}

impl PreviewResizeHandle {
    const fn direction(self) -> [f32; 2] {
        match self {
            Self::Left => [-1., 0.],
            Self::Right => [1., 0.],
            Self::Top => [0., -1.],
            Self::Bottom => [0., 1.],
        }
    }

    const fn cursor(self) -> CursorStyle {
        match self {
            Self::Left | Self::Right => CursorStyle::ResizeLeftRight,
            Self::Top | Self::Bottom => CursorStyle::ResizeUpDown,
        }
    }

    const fn changes_width(self) -> bool {
        matches!(self, Self::Left | Self::Right)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum PreviewEditTarget {
    Property,
    Keyframe(f32),
}

impl PreviewEditTarget {
    const fn from_progress(progress: Option<f32>) -> Self {
        match progress {
            Some(progress) => Self::Keyframe(progress),
            None => Self::Property,
        }
    }

    const fn key(self) -> u32 {
        match self {
            Self::Property => u32::MAX,
            Self::Keyframe(progress) => progress.to_bits(),
        }
    }

    const fn is_keyframe(self) -> bool {
        matches!(self, Self::Keyframe(_))
    }
}

#[derive(Clone, Copy)]
enum PreviewDragKind {
    Resize(PreviewResizeHandle),
    Position,
    Point(PropertyElementId),
}

#[derive(Clone)]
pub(super) struct PreviewEditorDrag {
    preview_id: EntityId,
    item_id: zerium_core::timeline::ItemId,
    effect_id: Option<EffectInstanceId>,
    kind: PreviewDragKind,
}

impl Render for PreviewEditorDrag {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        Empty
    }
}

#[derive(Clone)]
pub(super) struct PreviewResizeOrigin {
    item_id: zerium_core::timeline::ItemId,
    effect_id: Option<EffectInstanceId>,
    property_id: String,
    handle: PreviewResizeHandle,
    pointer: [f32; 2],
    size: [f32; 2],
    target: PreviewEditTarget,
    composition_units_per_pixel: f32,
}

#[derive(Clone)]
struct PreviewPositionOverlay {
    address: PropertyAddress,
    value: [f32; 2],
    target: PreviewEditTarget,
}

#[derive(Clone)]
struct PreviewPoint {
    element_id: PropertyElementId,
    value: [f32; 2],
    target: PreviewEditTarget,
}

#[derive(Clone)]
struct PreviewPointsProperty {
    property_id: String,
    value: PropertyValue,
}

#[derive(Clone)]
struct PreviewSizeOverlay {
    item_id: zerium_core::timeline::ItemId,
    property_id: String,
    center: [f32; 2],
    size: [f32; 2],
    target: PreviewEditTarget,
}

#[derive(Clone)]
struct PreviewPointOverlay {
    item_id: zerium_core::timeline::ItemId,
    center: [f32; 2],
    size: [f32; 2],
    points: PreviewPointsProperty,
    point: PreviewPoint,
    index: usize,
}

pub(super) struct PreviewEditorOverlay {
    effect_id: Option<EffectInstanceId>,
    positions: Vec<PreviewPositionOverlay>,
    sizes: Vec<PreviewSizeOverlay>,
    points: Vec<PreviewPointOverlay>,
    motion_path: Vec<[f32; 2]>,
    spline_path: Vec<[f32; 2]>,
}

#[derive(Clone)]
pub(super) struct PreviewPositionOrigin {
    address: PropertyAddress,
    pointer: [f32; 2],
    position: [f32; 2],
    target: PreviewEditTarget,
    composition_units_per_pixel: f32,
}

#[derive(Clone)]
pub(super) struct PreviewPointOrigin {
    item_id: zerium_core::timeline::ItemId,
    effect_id: Option<EffectInstanceId>,
    property_id: String,
    element_id: PropertyElementId,
    pointer: [f32; 2],
    point: [f32; 2],
    size: [f32; 2],
    value: PropertyValue,
    target: PreviewEditTarget,
    composition_units_per_pixel: f32,
}

#[derive(Default)]
pub(super) enum PreviewEditorDragState {
    #[default]
    Idle,
    Resize(PreviewResizeOrigin),
    Position(PreviewPositionOrigin),
    Point(PreviewPointOrigin),
}

impl PreviewEditorDragState {
    pub(super) fn clear(&mut self) -> bool {
        let was_active = !matches!(self, Self::Idle);
        *self = Self::default();
        was_active
    }
}

impl Preview {
    fn f32_pair(value: &PropertyValue) -> Option<[f32; 2]> {
        Some([
            value.scalar_at(Some(0))?.numeric_scalar()? as f32,
            value.scalar_at(Some(1))?.numeric_scalar()? as f32,
        ])
    }

    fn animation_progresses(
        item: &TimelineItem,
        effect_id: Option<EffectInstanceId>,
        property_id: &str,
        element_id: Option<PropertyElementId>,
    ) -> Vec<f32> {
        let mut progresses = (0..2)
            .filter_map(|scalar_index| {
                item.animation_track(effect_id, property_id, element_id, Some(scalar_index))
            })
            .flat_map(|track| track.stops().iter().map(|stop| stop.position()))
            .collect::<Vec<_>>();
        progresses.sort_by(f32::total_cmp);
        progresses.dedup_by(|left, right| (*left - *right).abs() <= 0.000_001);
        progresses
    }

    fn evaluated_at_progress(item: &TimelineItem, progress: f32) -> TimelineItem {
        item.evaluated_at_time(TimelineTime::from_frames(
            item.animation_timeline_frame(progress),
        ))
    }

    fn overlay_properties(
        item: &TimelineItem,
        effect_id: Option<EffectInstanceId>,
    ) -> Option<&PropertyValues> {
        match effect_id {
            Some(effect_id) => Some(
                &item
                    .effects
                    .iter()
                    .find(|effect| effect.id == effect_id)?
                    .properties,
            ),
            None => Some(&item.properties),
        }
    }

    fn item_position(
        item: &TimelineItem,
        effect_id: Option<EffectInstanceId>,
        property_id: &str,
    ) -> [f32; 2] {
        Self::overlay_properties(item, effect_id)
            .and_then(|properties| properties.property(property_id))
            .and_then(Self::f32_pair)
            .filter(|value| value.iter().all(|value| value.is_finite()))
            .unwrap_or([0., 0.])
    }

    fn position_at_progress(
        item: &TimelineItem,
        effect_id: Option<EffectInstanceId>,
        property_id: &str,
        progress: f32,
    ) -> Option<[f32; 2]> {
        let mut position =
            Self::f32_pair(Self::overlay_properties(item, effect_id)?.property(property_id)?)?;
        for (scalar_index, value) in position.iter_mut().enumerate() {
            if let Some(animated) = item
                .animation_track(effect_id, property_id, None, Some(scalar_index))
                .and_then(|track| track.evaluate(progress))
                .and_then(|value| value.numeric_scalar())
            {
                *value = animated as f32;
            }
        }
        position
            .iter()
            .all(|value| value.is_finite())
            .then_some(position)
    }

    fn item_size(
        item: &TimelineItem,
        effect_id: Option<EffectInstanceId>,
        property_id: &str,
    ) -> Option<[f32; 2]> {
        Self::f32_pair(Self::overlay_properties(item, effect_id)?.property(property_id)?)
            .filter(|value| value.iter().all(|value| value.is_finite() && *value > 0.))
    }
}

mod editing;
mod model;
mod render;
mod spline;
