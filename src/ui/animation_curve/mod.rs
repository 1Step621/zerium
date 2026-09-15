mod canvas;
mod curve;
mod editing;
mod grid;
mod render;
mod selection;
mod viewport;

pub(crate) use selection::{AnimationPresentation, AnimationSelection, AnimationTarget};

use std::cell::Cell;

use ::ui::{
    ActiveTheme as _, Colorize as _, Sizable as _,
    button::Button,
    menu::{PopupMenuItem, context_menu::ContextMenuExt as _},
};
use gpui::{
    App, Bounds, Context, Corner, Empty, Entity, FocusHandle, Hsla, MouseButton, MouseUpEvent,
    PathBuilder, Pixels, Render, Subscription, Window, canvas, div, point, prelude::*, px,
    relative,
};

use crate::domain::{
    animation::{BezierHandle, EasingDirection, EasingFamily, SegmentInterpolation},
    property::PropertyElementId,
    timeline::{
        EffectInstanceId, Frame, FrameDuration, FrameRate, ItemId, TimelineEditor, TimelineTime,
    },
};

use super::{
    inspector_path::InspectorPath,
    pane::pane_header,
    time_grid,
    transport::{ScrubSource, TransportController},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CurvePoint {
    HandleIn(usize),
    HandleOut(usize),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum GraphPressMode {
    EmptySpace,
    Curve,
}

#[derive(Clone, Copy, Debug)]
enum GraphInteraction {
    Idle,
    Background {
        origin: gpui::Point<Pixels>,
        mode: GraphPressMode,
        dragged: bool,
    },
    HandleDrag {
        point: CurvePoint,
    },
    StopDrag {
        stop: usize,
        frame: Frame,
        snap_frame: Frame,
        follow_focus: Option<usize>,
    },
    SuppressClick,
}

#[derive(Clone)]
struct CurvePointDrag {
    point: CurvePoint,
}

impl Render for CurvePointDrag {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        Empty
    }
}

#[derive(Clone)]
struct StopPositionDrag {
    stop: usize,
}

impl Render for StopPositionDrag {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        Empty
    }
}

#[derive(Clone)]
struct CurvePaintState {
    curve: GraphCurve,
    playhead_progress: f32,
    selected_segment: Option<usize>,
    grid: CurveGrid,
    grid_major: Hsla,
    grid_minor: Hsla,
    handle_color: Hsla,
    playhead_color: Hsla,
    curve_color: Hsla,
}

#[derive(Clone, Copy)]
struct CurvePaintColors {
    grid_major: Hsla,
    grid_minor: Hsla,
    handle: Hsla,
    playhead: Hsla,
    curve: Hsla,
}

#[derive(Clone)]
struct CurveGrid {
    major: Vec<(f64, f32)>,
    minor: Vec<f32>,
    values: Vec<(f64, f32)>,
}

#[derive(Clone)]
struct SelectedCurve {
    target: AnimationTarget,
    presentation: AnimationPresentation,
    animation: GraphAnimation,
    axis_suffix: String,
    source_segment: usize,
    source_stop_count: usize,
    source_stop_positions: Vec<f32>,
    source_playhead_progress: f32,
    playhead_progress: f32,
    clip_start: Frame,
    clip_duration: FrameDuration,
    animation_start_frame: f64,
    animation_span_frames: f64,
    start_seconds: f32,
    duration_seconds: f32,
    frame_rate: FrameRate,
}

#[derive(Clone)]
struct GraphAnimation {
    value_min: f64,
    value_max: f64,
    curve: GraphCurve,
}

#[derive(Clone)]
struct GraphCurve {
    stops: Vec<[f32; 2]>,
    interpolations: Vec<SegmentInterpolation>,
}

pub(crate) struct AnimationCurveEditor {
    editor: Entity<TimelineEditor>,
    transport: Entity<TransportController>,
    selection: Entity<AnimationSelection>,
    focus_handle: FocusHandle,
    graph_bounds: Option<Bounds<Pixels>>,
    selected_segment: Option<usize>,
    scrubbing_playhead: bool,
    graph_interaction: GraphInteraction,
    _subscriptions: Vec<Subscription>,
}

impl AnimationCurveEditor {
    const GRAPH_INSET_LEFT: f32 = 64.;
    const GRAPH_INSET_RIGHT: f32 = 24.;
    const GRAPH_INSET_TOP: f32 = 24.;
    const GRAPH_INSET_BOTTOM: f32 = 28.;
    const SCREEN_EDGE_EPSILON: f32 = 0.001;
    const SEGMENT_EDITOR_WIDTH: f32 = 240.;
    const SEGMENT_EDITOR_HEIGHT: f32 = 64.;
    const SEGMENT_EDITOR_GAP: f32 = 12.;
    const SEGMENT_EDITOR_PADDING: f32 = 8.;
    const OVERVIEW_STOP_HANDLE_WIDTH: f32 = 12.;
    const OVERVIEW_SNAP_DISTANCE: f32 = 8.;
    /// Pointer travel that promotes a graph press from "possible click" to a
    /// playhead scrub.
    const PRESS_DRAG_THRESHOLD_PX: f32 = 4.;

    pub(crate) fn new(
        editor: Entity<TimelineEditor>,
        transport: Entity<TransportController>,
        selection: Entity<AnimationSelection>,
        cx: &mut Context<Self>,
    ) -> Self {
        let subscriptions = vec![
            cx.observe(&editor, |_, _, cx| cx.notify()),
            cx.observe(&selection, |this, _, cx| {
                this.selected_segment = None;
                cx.notify();
            }),
            cx.observe(&transport, |_, _, cx| cx.notify()),
        ];
        Self {
            editor,
            transport,
            selection,
            focus_handle: cx.focus_handle(),
            graph_bounds: None,
            selected_segment: None,
            scrubbing_playhead: false,
            graph_interaction: GraphInteraction::Idle,
            _subscriptions: subscriptions,
        }
    }
}
