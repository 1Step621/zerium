use super::*;

fn squared_distance_to_line_segment(point: [f32; 2], start: [f32; 2], end: [f32; 2]) -> f32 {
    let delta = [end[0] - start[0], end[1] - start[1]];
    let length_squared = delta[0] * delta[0] + delta[1] * delta[1];
    if length_squared <= f32::EPSILON {
        return (point[0] - start[0]).powi(2) + (point[1] - start[1]).powi(2);
    }
    let offset = [point[0] - start[0], point[1] - start[1]];
    let progress = ((offset[0] * delta[0] + offset[1] * delta[1]) / length_squared).clamp(0., 1.);
    let closest = [
        start[0] + delta[0] * progress,
        start[1] + delta[1] * progress,
    ];
    (point[0] - closest[0]).powi(2) + (point[1] - closest[1]).powi(2)
}

impl AnimationCurveEditor {
    pub(super) fn graph_screen_position(&self, position: gpui::Point<Pixels>) -> Option<[f32; 2]> {
        let bounds = self.graph_bounds?;
        let width =
            f32::from(bounds.size.width - px(Self::GRAPH_INSET_LEFT + Self::GRAPH_INSET_RIGHT))
                .max(1.);
        let height =
            f32::from(bounds.size.height - px(Self::GRAPH_INSET_TOP + Self::GRAPH_INSET_BOTTOM))
                .max(1.);
        Some([
            (f32::from(position.x - bounds.origin.x) - Self::GRAPH_INSET_LEFT) / width,
            1. - (f32::from(position.y - bounds.origin.y) - Self::GRAPH_INSET_TOP) / height,
        ])
    }

    pub(super) fn point_layout(
        viewport: GraphViewport,
        position: [f32; 2],
        radius: f32,
    ) -> ([f32; 2], f32, f32) {
        let screen = viewport.screen_position(position).map(|value| {
            if value.abs() <= Self::SCREEN_EDGE_EPSILON {
                0.
            } else if (value - 1.).abs() <= Self::SCREEN_EDGE_EPSILON {
                1.
            } else {
                value
            }
        });
        (
            screen,
            Self::GRAPH_INSET_LEFT * (1. - screen[0])
                - Self::GRAPH_INSET_RIGHT * screen[0]
                - radius,
            Self::GRAPH_INSET_BOTTOM * (1. - screen[1])
                - Self::GRAPH_INSET_TOP * screen[1]
                - radius,
        )
    }

    pub(super) fn screen_position_is_visible(screen: [f32; 2]) -> bool {
        let visible = -Self::SCREEN_EDGE_EPSILON..=1. + Self::SCREEN_EDGE_EPSILON;
        visible.contains(&screen[0]) && visible.contains(&screen[1])
    }

    pub(super) fn point_editor_origin(graph_size: [f32; 2], screen: [f32; 2]) -> [f32; 2] {
        let plot_width = (graph_size[0] - Self::GRAPH_INSET_LEFT - Self::GRAPH_INSET_RIGHT).max(1.);
        let plot_height =
            (graph_size[1] - Self::GRAPH_INSET_TOP - Self::GRAPH_INSET_BOTTOM).max(1.);
        let point_x = Self::GRAPH_INSET_LEFT + plot_width * screen[0];
        let point_y = Self::GRAPH_INSET_TOP + plot_height * (1. - screen[1]);

        let left = if point_x <= graph_size[0] * 0.5 {
            point_x + Self::POINT_EDITOR_GAP
        } else {
            point_x - Self::POINT_EDITOR_GAP - Self::POINT_EDITOR_WIDTH
        };
        let top = if point_y <= graph_size[1] * 0.5 {
            point_y + Self::POINT_EDITOR_GAP
        } else {
            point_y - Self::POINT_EDITOR_GAP - Self::POINT_EDITOR_HEIGHT
        };
        let max_left = (graph_size[0] - Self::POINT_EDITOR_WIDTH - Self::POINT_EDITOR_PADDING)
            .max(Self::POINT_EDITOR_PADDING);
        let max_top = (graph_size[1] - Self::POINT_EDITOR_HEIGHT - Self::POINT_EDITOR_PADDING)
            .max(Self::POINT_EDITOR_PADDING);

        [
            left.clamp(Self::POINT_EDITOR_PADDING, max_left),
            top.clamp(Self::POINT_EDITOR_PADDING, max_top),
        ]
    }

    pub(super) fn normalized_position(&self, position: gpui::Point<Pixels>) -> Option<[f32; 2]> {
        let screen = self.graph_screen_position(position)?;
        let position = self.viewport.graph_position(screen);
        Some([position[0].clamp(0., 1.), position[1].clamp(0., 1.)])
    }

    pub(super) fn segment_at_position(
        &self,
        position: gpui::Point<Pixels>,
        cx: &App,
    ) -> Option<usize> {
        const HIT_RADIUS: f32 = 8.;
        const SAMPLES_PER_SEGMENT: usize = 48;

        let bounds = self.graph_bounds?;
        let selected = self.selected_curve(cx)?;
        let plot_width =
            f32::from(bounds.size.width) - Self::GRAPH_INSET_LEFT - Self::GRAPH_INSET_RIGHT;
        let plot_height =
            f32::from(bounds.size.height) - Self::GRAPH_INSET_TOP - Self::GRAPH_INSET_BOTTOM;
        if plot_width <= 0. || plot_height <= 0. {
            return None;
        }
        let pointer = [
            f32::from(position.x - bounds.origin.x),
            f32::from(position.y - bounds.origin.y),
        ];
        let to_pixel = |curve_position: [f32; 2]| {
            let screen = self.viewport.screen_position(curve_position);
            [
                Self::GRAPH_INSET_LEFT + plot_width * screen[0],
                Self::GRAPH_INSET_TOP + plot_height * (1. - screen[1]),
            ]
        };

        selected
            .animation
            .curve
            .anchors()
            .windows(2)
            .enumerate()
            .filter_map(|(segment, anchors)| {
                let start_progress = anchors[0][0];
                let end_progress = anchors[1][0];
                let mut previous = to_pixel([
                    start_progress,
                    selected.animation.curve.evaluate(start_progress),
                ]);
                let mut distance = f32::INFINITY;
                for step in 1..=SAMPLES_PER_SEGMENT {
                    let progress = start_progress
                        + (end_progress - start_progress) * step as f32
                            / SAMPLES_PER_SEGMENT as f32;
                    let current = to_pixel([progress, selected.animation.curve.evaluate(progress)]);
                    distance =
                        distance.min(squared_distance_to_line_segment(pointer, previous, current));
                    previous = current;
                }
                (distance <= HIT_RADIUS * HIT_RADIUS).then_some((segment, distance))
            })
            .min_by(|left, right| left.1.total_cmp(&right.1))
            .map(|(segment, _)| segment)
    }

    fn set_selected_segment(
        &mut self,
        segment: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.editor
            .update(cx, |editor, _| editor.finish_history_group());
        self.selected_point = None;
        self.selected_segment = Some(segment);
        self.sync_point_inputs(window, cx);
        self.focus_handle.focus(window, cx);
        cx.notify();
    }

    /// Records a graph-background press. Intentionally side-effect free:
    /// selection and scrubbing are decided later by movement (`graph_press_moved`)
    /// or click resolution (`resolve_graph_click` / `graph_double_click`).
    pub(super) fn graph_press_started(&mut self, position: gpui::Point<Pixels>) {
        self.press_origin = Some(position);
        self.press_dragged = false;
    }

    /// Promotes the press to a playhead scrub once the pointer travels past
    /// the drag threshold; smaller movements stay click candidates so plain
    /// and double clicks never move the playhead.
    pub(super) fn graph_press_moved(
        &mut self,
        event: &gpui::MouseMoveEvent,
        cx: &mut Context<Self>,
    ) {
        // Mouse-up can be lost outside the window. A hover must never continue
        // or promote a stale press into a scrub when the pointer returns.
        if event.pressed_button != Some(MouseButton::Left) {
            self.end_graph_press(cx);
            return;
        }
        let position = event.position;
        let Some(origin) = self.press_origin else {
            return;
        };
        if self.scrubbing_playhead {
            self.seek_playhead_from_graph(position, cx);
            return;
        }
        let delta = [
            f32::from(position.x - origin.x),
            f32::from(position.y - origin.y),
        ];
        if delta[0].hypot(delta[1]) >= Self::PRESS_DRAG_THRESHOLD_PX {
            self.press_dragged = true;
            self.scrubbing_playhead = true;
            self.transport.update(cx, |transport, cx| {
                transport.begin_scrub(ScrubSource::AnimationCurve, cx);
            });
            self.seek_playhead_from_graph(position, cx);
        }
    }

    /// Ends the press. Runs on mouse-up and as a safety net for releases
    /// outside the graph; `press_dragged` is deliberately kept so the click
    /// event following a scrub drag can still be suppressed. It is reset by
    /// the next press or consumed by click resolution.
    pub(super) fn end_graph_press(&mut self, cx: &mut Context<Self>) {
        self.press_origin = None;
        self.finish_playhead_scrub(cx);
    }

    /// Single-click resolution with exactly one hit test: a drag-turned-scrub
    /// is swallowed, otherwise the hit segment is selected or the selection
    /// is cleared. Never runs on mousedown, so the floating panel cannot
    /// appear under the second click of a double-click.
    pub(super) fn resolve_graph_click(
        &mut self,
        position: gpui::Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.press_dragged {
            self.press_dragged = false;
            self.press_origin = None;
            return;
        }
        self.press_origin = None;
        if let Some(segment) = self.segment_at_position(position, cx) {
            self.set_selected_segment(segment, window, cx);
        } else {
            self.clear_selection(window, cx);
        }
    }

    /// Double-click resolution: adds an anchor without ever starting a scrub,
    /// so the playhead stays put.
    pub(super) fn graph_double_click(
        &mut self,
        position: gpui::Point<Pixels>,
        snap_disabled: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.press_origin = None;
        self.press_dragged = false;
        self.finish_playhead_scrub(cx);
        self.add_anchor(position, snap_disabled, window, cx);
    }

    /// Placement anchor for the selected-segment floating panel.
    ///
    /// Unlike the raw segment midpoint this never leaves the drawable area:
    /// x falls back to the center of the segment/viewport intersection and
    /// only the placement y is clamped, so zoomed-out midpoints and
    /// overshooting easings (e.g. Elastic) keep their menu.
    pub(super) fn segment_panel_position(
        curve: &AnimationCurve,
        segment: usize,
        viewport: GraphViewport,
    ) -> Option<[f32; 2]> {
        let end = segment.checked_add(1)?;
        let anchors = curve.anchors().get(segment..=end)?;
        let visible_start = anchors[0][0].max(viewport.x_min);
        let visible_end = anchors[1][0].min(viewport.x_max);
        let (visible_start, visible_end) = if visible_start <= visible_end {
            (visible_start, visible_end)
        } else {
            let edge = anchors[0][0].clamp(viewport.x_min, viewport.x_max);
            (edge, edge)
        };
        let midpoint = (anchors[0][0] + anchors[1][0]) * 0.5;
        let progress = if (visible_start..=visible_end).contains(&midpoint) {
            midpoint
        } else {
            (visible_start + visible_end) * 0.5
        };
        Some([progress, curve.evaluate(progress).clamp(0., 1.)])
    }

    pub(super) fn frame_at_progress(selected: &SelectedCurve, progress: f32) -> Frame {
        let clip_start = selected.clip_start.get() as f64;
        let clip_end = selected
            .clip_start
            .get()
            .saturating_add(selected.clip_duration.get().saturating_sub(1))
            as f64;
        let frame = selected.animation_start_frame
            + f64::from(progress.clamp(0., 1.)) * selected.animation_span_frames;
        Frame::new(frame.round().clamp(clip_start, clip_end) as u64)
    }

    pub(super) fn seek_playhead_from_graph(
        &mut self,
        position: gpui::Point<Pixels>,
        cx: &mut Context<Self>,
    ) {
        let Some(screen) = self.graph_screen_position(position) else {
            return;
        };
        let progress = self.viewport.graph_position(screen)[0].clamp(0., 1.);
        let Some(selected) = self.selected_curve(cx) else {
            return;
        };
        let frame = Self::frame_at_progress(&selected, progress);
        self.transport
            .update(cx, |transport, cx| transport.set_playhead(frame, cx));
    }

    pub(super) fn finish_playhead_scrub(&mut self, cx: &mut Context<Self>) {
        if !self.scrubbing_playhead {
            return;
        }
        self.scrubbing_playhead = false;
        self.transport.update(cx, |transport, cx| {
            transport.end_scrub(ScrubSource::AnimationCurve, cx);
        });
    }

    pub(super) fn zoom(&mut self, factor: f32, anchor: f32, cx: &mut Context<Self>) {
        if self.viewport.zoom(factor, anchor) {
            cx.notify();
        }
    }

    pub(super) fn scroll_graph(
        &mut self,
        event: &ScrollWheelEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let delta = event.delta.pixel_delta(window.line_height());
        let x = f32::from(delta.x);
        let y = f32::from(delta.y);
        if event.modifiers.control {
            let dominant = if x.abs() > y.abs() { x } else { y };
            if dominant != 0. {
                let anchor = self
                    .graph_screen_position(event.position)
                    .map_or(0.5, |position| position[0].clamp(0., 1.));
                let factor = if dominant > 0. {
                    Self::ZOOM_FACTOR
                } else {
                    1. / Self::ZOOM_FACTOR
                };
                self.zoom(factor, anchor, cx);
            }
        } else if let Some(bounds) = self.graph_bounds {
            let width = f32::from(bounds.size.width).max(1.);
            let horizontal = if x.abs() > y.abs() { x } else { y };
            if self.viewport.pan(horizontal / width) {
                cx.notify();
            }
        }
        cx.stop_propagation();
    }
}
