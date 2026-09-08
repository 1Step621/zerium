mod canvas;
mod curve;
mod editing;
mod grid;
mod render;
mod selection;
mod viewport;

pub(crate) use selection::{AnimationPresentation, AnimationSelection, AnimationTarget};

use ::ui::{
    ActiveTheme as _, Disableable as _, Sizable as _,
    button::Button,
    input::{InputEvent, InputState, NumberInput, NumberInputEvent, StepAction},
    menu::{PopupMenuItem, context_menu::ContextMenuExt as _},
};
use gpui::{
    App, Bounds, Context, Corner, CursorStyle, Empty, Entity, EntityId, FocusHandle, Hsla,
    MouseButton, MouseDownEvent, MouseUpEvent, PathBuilder, Pixels, Render, ScrollWheelEvent,
    Subscription, Window, canvas, div, point, prelude::*, px, relative,
};

use crate::domain::{
    animation::{
        AnimationCurve, BezierHandle, EasingDirection, EasingFamily, ParameterAnimationAddress,
        SegmentInterpolation,
    },
    timeline::{
        EffectInstanceId, Frame, FrameDuration, FrameRate, ItemId, TimelineEditor, TimelineTime,
    },
};

use super::{
    pane::pane_header,
    property::PropertyPath,
    time_grid,
    transport::{ScrubSource, TransportController},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CurvePoint {
    Anchor(usize),
    HandleIn(usize),
    HandleOut(usize),
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
struct CurveValueDrag {
    editor_id: EntityId,
}

impl Render for CurveValueDrag {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        Empty
    }
}

#[derive(Clone, Copy)]
struct CurveValueDragOrigin {
    start_x: f32,
    start_value: f64,
    min: f64,
    max: f64,
    step: f64,
    sensitivity: f64,
}

#[derive(Clone)]
struct CurvePaintState {
    curve: AnimationCurve,
    background_curves: Vec<AnimationCurve>,
    playhead_progress: f32,
    visible_progress_range: [f32; 2],
    selected_segment: Option<usize>,
    viewport: GraphViewport,
    grid: CurveGrid,
    grid_major: Hsla,
    grid_minor: Hsla,
    handle_color: Hsla,
    playhead_color: Hsla,
    curve_color: Hsla,
    background_curve_color: Hsla,
}

#[derive(Clone, Copy)]
struct CurveTimeView {
    playhead_progress: f32,
    visible_progress_range: [f32; 2],
}

#[derive(Clone, Copy)]
struct CurvePaintColors {
    grid_major: Hsla,
    grid_minor: Hsla,
    handle: Hsla,
    playhead: Hsla,
    curve: Hsla,
    background_curve: Hsla,
}

#[derive(Clone)]
struct CurveGrid {
    major: Vec<(f64, f32)>,
    minor: Vec<f32>,
    values: Vec<(f64, f32)>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct GraphViewport {
    x_min: f32,
    x_max: f32,
}

impl Default for GraphViewport {
    fn default() -> Self {
        Self {
            x_min: 0.,
            x_max: 1.,
        }
    }
}

impl GraphViewport {
    const MIN_SPAN: f32 = 0.04;

    fn x_span(self) -> f32 {
        self.x_max - self.x_min
    }

    fn screen_position(self, position: [f32; 2]) -> [f32; 2] {
        [(position[0] - self.x_min) / self.x_span(), position[1]]
    }

    fn graph_position(self, screen: [f32; 2]) -> [f32; 2] {
        [self.x_min + screen[0] * self.x_span(), screen[1]]
    }

    fn zoom(&mut self, factor: f32, anchor: f32) -> bool {
        let old = *self;
        let x_span = (self.x_span() / factor).clamp(Self::MIN_SPAN, 1.);
        let anchor_x = self.x_min + anchor * self.x_span();
        self.x_min = anchor_x - anchor * x_span;
        self.x_max = self.x_min + x_span;
        self.clamp();
        *self != old
    }

    fn pan(&mut self, screen_delta: f32) -> bool {
        let old = *self;
        let x_shift = screen_delta * self.x_span();
        self.x_min += x_shift;
        self.x_max += x_shift;
        self.clamp();
        *self != old
    }

    fn clamp(&mut self) {
        let x_span = self.x_span();
        self.x_min = self.x_min.clamp(0., 1. - x_span);
        self.x_max = self.x_min + x_span;
    }
}

#[derive(Clone)]
struct SelectedCurve {
    target: AnimationTarget,
    presentation: AnimationPresentation,
    animation: GraphAnimation,
    axis_suffix: String,
    background_curves: Vec<AnimationCurve>,
    playhead_progress: f32,
    clip_start: Frame,
    clip_duration: FrameDuration,
    animation_start_frame: f64,
    animation_span_frames: f64,
    visible_progress_range: [f32; 2],
    start_seconds: f32,
    duration_seconds: f32,
    frame_rate: FrameRate,
}

#[derive(Clone)]
struct GraphAnimation {
    from: f64,
    to: f64,
    curve: AnimationCurve,
}

pub(crate) struct AnimationCurveEditor {
    editor: Entity<TimelineEditor>,
    transport: Entity<TransportController>,
    selection: Entity<AnimationSelection>,
    focus_handle: FocusHandle,
    graph_bounds: Option<Bounds<Pixels>>,
    selected_point: Option<CurvePoint>,
    selected_segment: Option<usize>,
    viewport: GraphViewport,
    point_value_input: Entity<InputState>,
    syncing_point_inputs: bool,
    scrubbing_playhead: bool,
    /// Position of the ongoing graph-background press, if any.
    ///
    /// The press is only a *candidate* until it resolves: past the drag
    /// threshold it becomes a playhead scrub, otherwise click resolution
    /// (segment select / deselect / anchor add) owns it. Recording the
    /// origin here keeps mousedown / mousemove / click / double-click as a
    /// single gesture instead of competing side effects.
    press_origin: Option<gpui::Point<Pixels>>,
    /// True once the ongoing press turned into a scrub drag. The click event
    /// that follows mouse-up must then be swallowed so a scrub never changes
    /// the selection.
    press_dragged: bool,
    value_drag_origin: Option<CurveValueDragOrigin>,
    _subscriptions: Vec<Subscription>,
}

impl AnimationCurveEditor {
    const GRAPH_INSET_LEFT: f32 = 64.;
    const GRAPH_INSET_RIGHT: f32 = 24.;
    const GRAPH_INSET_TOP: f32 = 24.;
    const GRAPH_INSET_BOTTOM: f32 = 28.;
    const SCREEN_EDGE_EPSILON: f32 = 0.001;
    const POINT_EDITOR_WIDTH: f32 = 240.;
    const POINT_EDITOR_HEIGHT: f32 = 64.;
    const POINT_EDITOR_GAP: f32 = 12.;
    const POINT_EDITOR_PADDING: f32 = 8.;
    const ZOOM_FACTOR: f32 = 1.2;
    /// Pointer travel that promotes a graph press from "possible click" to a
    /// playhead scrub.
    const PRESS_DRAG_THRESHOLD_PX: f32 = 4.;

    pub(crate) fn new(
        editor: Entity<TimelineEditor>,
        transport: Entity<TransportController>,
        selection: Entity<AnimationSelection>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let point_value_input = cx.new(|cx| InputState::new(window, cx));
        let mut subscriptions = vec![
            cx.observe_in(&editor, window, |this, _, window, cx| {
                this.sync_point_inputs(window, cx);
                cx.notify();
            }),
            cx.observe_in(&selection, window, |this, _, window, cx| {
                this.selected_point = None;
                this.selected_segment = None;
                this.viewport = GraphViewport::default();
                this.sync_point_inputs(window, cx);
                cx.notify();
            }),
            cx.observe(&transport, |_, _, cx| cx.notify()),
        ];
        subscriptions.push(cx.subscribe_in(
            &point_value_input,
            window,
            |this, input, event, window, cx| {
                this.handle_point_input_change(input, event, window, cx);
            },
        ));
        subscriptions.push(cx.subscribe_in(
            &point_value_input,
            window,
            |this, input, event, window, cx| {
                this.handle_point_input_step(input, event, window, cx);
            },
        ));
        let mut this = Self {
            editor,
            transport,
            selection,
            focus_handle: cx.focus_handle(),
            graph_bounds: None,
            selected_point: None,
            selected_segment: None,
            viewport: GraphViewport::default(),
            point_value_input,
            syncing_point_inputs: false,
            scrubbing_playhead: false,
            press_origin: None,
            press_dragged: false,
            value_drag_origin: None,
            _subscriptions: subscriptions,
        };
        this.sync_point_inputs(window, cx);
        this
    }
}
