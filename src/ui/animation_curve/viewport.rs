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
    pub(super) fn graph_stop_at_position(
        &self,
        position: gpui::Point<Pixels>,
        cx: &App,
    ) -> Option<usize> {
        const HIT_RADIUS: f32 = 10.;
        let bounds = self.graph_bounds?;
        let selected = self.selected_curve(cx)?;
        let plot_width =
            f32::from(bounds.size.width) - Self::GRAPH_INSET_LEFT - Self::GRAPH_INSET_RIGHT;
        let plot_height =
            f32::from(bounds.size.height) - Self::GRAPH_INSET_TOP - Self::GRAPH_INSET_BOTTOM;
        let pointer = [
            f32::from(position.x - bounds.origin.x),
            f32::from(position.y - bounds.origin.y),
        ];
        selected
            .animation
            .curve
            .stops()
            .iter()
            .enumerate()
            .find_map(|(index, stop)| {
                let point = [
                    Self::GRAPH_INSET_LEFT + plot_width * stop[0],
                    Self::GRAPH_INSET_TOP + plot_height * (1. - stop[1]),
                ];
                ((pointer[0] - point[0]).hypot(pointer[1] - point[1]) <= HIT_RADIUS)
                    .then_some(index)
            })
    }

    pub(super) fn overview_stop_at_position(
        &self,
        position: gpui::Point<Pixels>,
        cx: &App,
    ) -> Option<usize> {
        let bounds = self.graph_bounds?;
        let selected = self.selected_curve(cx)?;
        let plot_left = f32::from(bounds.origin.x) + Self::GRAPH_INSET_LEFT;
        let plot_width =
            f32::from(bounds.size.width) - Self::GRAPH_INSET_LEFT - Self::GRAPH_INSET_RIGHT;
        if plot_width <= 0. {
            return None;
        }
        let pointer_x = f32::from(position.x);
        selected
            .source_stop_positions
            .iter()
            .enumerate()
            .map(|(index, progress)| {
                let stop_x = plot_left + plot_width * progress.clamp(0., 1.);
                (index, (pointer_x - stop_x).abs())
            })
            .filter(|(_, distance)| *distance <= Self::OVERVIEW_STOP_HANDLE_WIDTH * 0.5)
            .min_by(|left, right| left.1.total_cmp(&right.1))
            .map(|(index, _)| index)
    }

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

    pub(super) fn point_layout(position: [f32; 2], radius: f32) -> ([f32; 2], f32, f32) {
        let screen = position.map(|value| {
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

    pub(super) fn segment_editor_origin(graph_size: [f32; 2], screen: [f32; 2]) -> [f32; 2] {
        let plot_width = (graph_size[0] - Self::GRAPH_INSET_LEFT - Self::GRAPH_INSET_RIGHT).max(1.);
        let plot_height =
            (graph_size[1] - Self::GRAPH_INSET_TOP - Self::GRAPH_INSET_BOTTOM).max(1.);
        let point_x = Self::GRAPH_INSET_LEFT + plot_width * screen[0];
        let point_y = Self::GRAPH_INSET_TOP + plot_height * (1. - screen[1]);

        let left = if point_x <= graph_size[0] * 0.5 {
            point_x + Self::SEGMENT_EDITOR_GAP
        } else {
            point_x - Self::SEGMENT_EDITOR_GAP - Self::SEGMENT_EDITOR_WIDTH
        };
        let top = if point_y <= graph_size[1] * 0.5 {
            point_y + Self::SEGMENT_EDITOR_GAP
        } else {
            point_y - Self::SEGMENT_EDITOR_GAP - Self::SEGMENT_EDITOR_HEIGHT
        };
        let max_left = (graph_size[0] - Self::SEGMENT_EDITOR_WIDTH - Self::SEGMENT_EDITOR_PADDING)
            .max(Self::SEGMENT_EDITOR_PADDING);
        let max_top = (graph_size[1] - Self::SEGMENT_EDITOR_HEIGHT - Self::SEGMENT_EDITOR_PADDING)
            .max(Self::SEGMENT_EDITOR_PADDING);

        [
            left.clamp(Self::SEGMENT_EDITOR_PADDING, max_left),
            top.clamp(Self::SEGMENT_EDITOR_PADDING, max_top),
        ]
    }

    pub(super) fn normalized_position(&self, position: gpui::Point<Pixels>) -> Option<[f32; 2]> {
        let screen = self.graph_screen_position(position)?;
        Some([screen[0].clamp(0., 1.), screen[1].clamp(0., 1.)])
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
            [
                Self::GRAPH_INSET_LEFT + plot_width * curve_position[0],
                Self::GRAPH_INSET_TOP + plot_height * (1. - curve_position[1]),
            ]
        };

        selected
            .animation
            .curve
            .stops()
            .windows(2)
            .enumerate()
            .filter_map(|(segment, stops)| {
                let start_progress = stops[0][0];
                let end_progress = stops[1][0];
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

    fn set_selected_segment(&mut self, segment: usize, cx: &mut Context<Self>) {
        self.editor
            .update(cx, |editor, _| editor.finish_history_group());
        self.selected_segment = Some(segment);
        cx.notify();
    }

    pub(super) fn focus_source_segment(&mut self, segment: usize, cx: &mut Context<Self>) {
        let Some(selected) = self.selected_curve(cx) else {
            return;
        };
        let Some(progress) = selected.source_stop_positions.get(segment).copied() else {
            return;
        };
        let Some(frame) = self
            .editor
            .read(cx)
            .item(selected.target.item_id)
            .map(|item| Frame::new(item.animation_timeline_frame(progress).round().max(0.) as u64))
        else {
            return;
        };
        self.editor
            .update(cx, |editor, _| editor.finish_history_group());
        self.selection.read(cx).focus_segment(segment);
        self.selected_segment = None;
        self.transport
            .update(cx, |transport, cx| transport.seek(frame, cx));
        cx.notify();
    }

    /// Records a graph-background press. Empty space seeks at once so the
    /// playhead tracks the press instead of waiting for mouse-up; a scrub
    /// session opens only while playing, mirroring the timeline ruler.
    /// Segment presses stay side-effect free click candidates.
    pub(super) fn graph_press_started(
        &mut self,
        position: gpui::Point<Pixels>,
        cx: &mut Context<Self>,
    ) {
        let in_plot = self
            .graph_screen_position(position)
            .is_some_and(|screen| (0. ..=1.).contains(&screen[0]));
        if !in_plot {
            self.graph_interaction = GraphInteraction::Idle;
            return;
        }
        let mode = if self.segment_at_position(position, cx).is_some() {
            GraphPressMode::Curve
        } else {
            GraphPressMode::EmptySpace
        };
        self.graph_interaction = GraphInteraction::Background {
            origin: position,
            mode,
            dragged: false,
        };
        if mode != GraphPressMode::EmptySpace {
            return;
        }
        if self.transport.read(cx).is_playing() {
            self.scrubbing_playhead = true;
            self.transport.update(cx, |transport, cx| {
                transport.begin_scrub(ScrubSource::AnimationCurve, cx);
            });
        }
        if let Some(frame) = self.frame_at_graph_position(position, cx) {
            self.transport
                .update(cx, |transport, cx| transport.set_playhead(frame, cx));
        }
    }

    /// Tracks the press: past the drag threshold it counts as a drag (the
    /// following click is swallowed) and scrubbing is ensured; scrubbing
    /// presses keep seeking on every move.
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
        let GraphInteraction::Background {
            origin,
            mode: GraphPressMode::EmptySpace,
            dragged,
        } = self.graph_interaction
        else {
            return;
        };
        let position = event.position;
        let delta = [
            f32::from(position.x - origin.x),
            f32::from(position.y - origin.y),
        ];
        if !dragged && delta[0].hypot(delta[1]) >= Self::PRESS_DRAG_THRESHOLD_PX {
            self.graph_interaction = GraphInteraction::Background {
                origin,
                mode: GraphPressMode::EmptySpace,
                dragged: true,
            };
            if !self.scrubbing_playhead {
                self.scrubbing_playhead = true;
                self.transport.update(cx, |transport, cx| {
                    transport.begin_scrub(ScrubSource::AnimationCurve, cx);
                });
            }
        }
        if self.scrubbing_playhead {
            self.seek_playhead_from_graph(position, cx);
        }
    }

    /// Ends a background press. A completed scrub becomes `SuppressClick` so
    /// the click event following mouse-up cannot change the selection.
    pub(super) fn end_graph_press(&mut self, cx: &mut Context<Self>) {
        self.graph_interaction = match self.graph_interaction {
            GraphInteraction::Background { dragged: true, .. } => GraphInteraction::SuppressClick,
            GraphInteraction::Background { .. } => GraphInteraction::Idle,
            interaction => interaction,
        };
        self.finish_playhead_scrub(cx);
    }

    /// A drag-turned-scrub is swallowed; otherwise the displayed segment is
    /// selected or the playhead seeks to the clicked position.
    pub(super) fn resolve_graph_click(
        &mut self,
        position: gpui::Point<Pixels>,
        cx: &mut Context<Self>,
    ) {
        if matches!(self.graph_interaction, GraphInteraction::SuppressClick) {
            self.graph_interaction = GraphInteraction::Idle;
            return;
        }
        self.graph_interaction = GraphInteraction::Idle;
        if let Some(segment) = self.segment_at_position(position, cx) {
            self.set_selected_segment(segment, cx);
        } else {
            self.clear_selection(cx);
            let in_plot = self
                .graph_screen_position(position)
                .is_some_and(|screen| (0. ..=1.).contains(&screen[0]));
            if in_plot && let Some(frame) = self.frame_at_graph_position(position, cx) {
                self.transport
                    .update(cx, |transport, cx| transport.seek(frame, cx));
            }
        }
    }

    /// Placement point for the selected-segment floating panel.
    pub(super) fn segment_panel_position(curve: &GraphCurve, segment: usize) -> Option<[f32; 2]> {
        let end = segment.checked_add(1)?;
        let stops = curve.stops().get(segment..=end)?;
        let midpoint = (stops[0][0] + stops[1][0]) * 0.5;
        Some([midpoint, curve.evaluate(midpoint).clamp(0., 1.)])
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

    pub(super) fn frame_at_source_progress(selected: &SelectedCurve, progress: f32) -> Frame {
        let span_frames = selected.clip_duration.get().saturating_sub(1);
        Frame::new(selected.clip_start.get().saturating_add(
            (f64::from(progress.clamp(0., 1.)) * span_frames as f64).round() as u64,
        ))
    }

    pub(super) fn frame_at_graph_position(
        &self,
        position: gpui::Point<Pixels>,
        cx: &App,
    ) -> Option<Frame> {
        let screen = self.graph_screen_position(position)?;
        let progress = screen[0].clamp(0., 1.);
        let selected = self.selected_curve(cx)?;
        Some(Self::frame_at_progress(&selected, progress))
    }

    pub(super) fn frame_at_overview_position(
        &self,
        position: gpui::Point<Pixels>,
        cx: &App,
    ) -> Option<Frame> {
        let progress = self.graph_screen_position(position)?[0].clamp(0., 1.);
        let selected = self.selected_curve(cx)?;
        Some(Self::frame_at_source_progress(&selected, progress))
    }

    pub(super) fn seek_playhead_from_graph(
        &mut self,
        position: gpui::Point<Pixels>,
        cx: &mut Context<Self>,
    ) {
        if let Some(frame) = self.frame_at_graph_position(position, cx) {
            self.transport
                .update(cx, |transport, cx| transport.set_playhead(frame, cx));
        }
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
}
