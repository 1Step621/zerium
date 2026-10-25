mod canvas;
mod curve;
mod editing;
mod grid;
mod presentation;
mod render;
mod repeat;
mod selection;
mod steps;
mod viewport;

use canvas::graph_canvas;
use curve::format_number;
use grid::{normalized_value, value_grid};
use viewport::{point_layout, screen_position_is_visible};

use presentation::AnimationPresentation;
pub(crate) use selection::AnimationSelection;

use crate::ui::numeric_property::NumericDrag;
use ::ui::input::InputState;
use repeat::{RepeatInputs, RepeatParameter};
use std::cell::Cell;
use steps::StepInput;

use ::ui::{
    ActiveTheme as _, Colorize as _, Icon, IconName, Sizable as _,
    button::Button,
    menu::{PopupMenuItem, context_menu::ContextMenuExt as _},
};
use gpui::{
    App, Bounds, Context, Corner, Empty, Entity, FocusHandle, Hsla, MouseButton, MouseUpEvent,
    PathBuilder, Pixels, Render, Subscription, Window, canvas, div, point, prelude::*, px,
    relative,
};

use zerium_core::{
    animation::{
        AnimationClock, AnimationRepeat, BezierHandle, EasingDirection, EasingFamily, RepeatMode,
        SegmentInterpolation,
    },
    timeline::{
        AnimationEdit, AnimationEditTarget, BeatGuide, Frame, FrameDuration, FrameRate,
        PropertyAddress, TimelineEditor, TimelineTime,
    },
};

use super::{
    pane::pane_header,
    time_grid,
    transport::{ScrubSource, TransportController},
};

enum GraphInteraction {
    Idle,
    Background {
        origin: gpui::Point<Pixels>,
        dragged: bool,
    },
    HandleDrag {
        handle: BezierHandle,
        edit: Box<AnimationEdit>,
        // Freeze the interval and value range throughout this drag.
        view: Box<SelectedCurve>,
    },
    StopDrag {
        stop: usize,
        snap_playhead: Frame,
        edit: Box<AnimationEdit>,
    },
}

#[derive(Clone, Copy)]
enum CurveNumberParameter {
    StepCount,
    Repeat(RepeatParameter),
}

struct CurveNumberDrag {
    parameter: CurveNumberParameter,
    input: Entity<InputState>,
    motion: NumericDrag,
}

#[derive(Clone)]
struct CurveHandleDrag {
    handle: BezierHandle,
}

impl Render for CurveHandleDrag {
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
    curve: GraphSegment,
    curve_width: Pixels,
    playhead_progress: f32,
    grid: CurveGrid,
    colors: CurvePaintColors,
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
    ruler_ticks: Vec<(f64, f32)>,
    beat_ticks: Vec<f32>,
    frame_ticks: Vec<f32>,
    values: Vec<(f64, f32)>,
}

#[derive(Clone)]
struct SelectedCurve {
    address: PropertyAddress,
    presentation: AnimationPresentation,
    value_min: f64,
    value_max: f64,
    curve: GraphSegment,
    axis_suffix: String,
    source_segment: usize,
    source_stop_frames: Vec<Frame>,
    source_playhead_progress: f32,
    playhead_progress: f32,
    clip_start: Frame,
    clip_duration: FrameDuration,
    clock: AnimationClock,
    repeat: AnimationRepeat,

    start_seconds: f32,
    duration_seconds: f32,
    frame_rate: FrameRate,
}

#[derive(Clone)]
struct GraphSegment {
    endpoints: [[f32; 2]; 2],
    interpolation: SegmentInterpolation,
}

/// Identity of an authored interval, independent of its interpolation settings.
struct SegmentIdentity {
    address: PropertyAddress,
    source_segment: usize,
    source_frames: [Frame; 2],
}

pub(crate) struct AnimationCurveEditor {
    editor: Entity<TimelineEditor>,
    transport: Entity<TransportController>,
    selection: Entity<AnimationSelection>,
    focus_handle: FocusHandle,
    graph_bounds: Option<Bounds<Pixels>>,
    graph_interaction: GraphInteraction,
    handle_fit_target: Option<SegmentIdentity>,
    repeat_inputs: Option<RepeatInputs>,
    step_input: Option<StepInput>,
    number_drag: Option<CurveNumberDrag>,
    _subscriptions: Vec<Subscription>,
}

impl AnimationCurveEditor {
    const GRAPH_INSET_LEFT: f32 = 64.;
    const GRAPH_INSET_RIGHT: f32 = 24.;
    const GRAPH_INSET_TOP: f32 = 24.;
    const GRAPH_INSET_BOTTOM: f32 = 28.;
    const SCREEN_EDGE_EPSILON: f32 = 0.001;
    const OVERVIEW_STOP_HANDLE_WIDTH: f32 = 16.;
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
            cx.observe(&editor, |this, _, cx| {
                this.discard_stale_edits(cx);
                cx.notify();
            }),
            cx.observe(&selection, |this, _, cx| {
                this.end_pointer_drag(cx);
                this.end_graph_press(cx);
                this.handle_fit_target = None;
                this.repeat_inputs = None;
                this.step_input = None;
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
            graph_interaction: GraphInteraction::Idle,
            handle_fit_target: None,
            repeat_inputs: None,
            step_input: None,
            number_drag: None,
            _subscriptions: subscriptions,
        }
    }
}
