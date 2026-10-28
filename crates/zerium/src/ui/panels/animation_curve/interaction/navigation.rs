use crate::ui::panels::animation_curve::*;

impl AnimationCurveEditor {
    pub(in crate::ui::panels::animation_curve) fn graph_stop_at_position(
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
            .curve
            .endpoints
            .iter()
            .enumerate()
            .find_map(|(index, stop)| {
                let point = [
                    Self::GRAPH_INSET_LEFT + plot_width * stop[0],
                    Self::GRAPH_INSET_TOP + plot_height * (1. - stop[1]),
                ];
                ((pointer[0] - point[0]).hypot(pointer[1] - point[1]) <= HIT_RADIUS)
                    .then_some(selected.source_segment + index)
            })
    }

    pub(in crate::ui::panels::animation_curve) fn overview_stop_at_position(
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
            .stop_positions()
            .enumerate()
            .map(|(index, progress)| {
                let stop_x = plot_left + plot_width * progress.clamp(0., 1.);
                (index, (pointer_x - stop_x).abs())
            })
            .filter(|(_, distance)| *distance <= Self::OVERVIEW_STOP_HANDLE_WIDTH * 0.5)
            .min_by(|left, right| left.1.total_cmp(&right.1))
            .map(|(index, _)| index)
    }

    pub(in crate::ui::panels::animation_curve) fn graph_screen_position(
        &self,
        position: gpui::Point<Pixels>,
    ) -> Option<[f32; 2]> {
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

    pub(in crate::ui::panels::animation_curve) fn focus_source_segment(
        &mut self,
        segment: usize,
        cx: &mut Context<Self>,
    ) {
        let Some(selected) = self.selected_curve(cx) else {
            return;
        };
        let Some(pattern_frame) = selected.source_stop_frames.get(segment).copied() else {
            return;
        };
        let Some(frame) = selected.frame_at_pattern_frame(pattern_frame) else {
            return;
        };
        self.editor
            .update(cx, |editor, _| editor.finish_history_group());
        self.selection.read(cx).focus_segment(segment);
        self.transport
            .update(cx, |transport, cx| transport.set_playhead(frame, cx));
        cx.notify();
    }

    /// Seek on press; while playing, suspend playback until release.
    pub(in crate::ui::panels::animation_curve) fn graph_press_started(
        &mut self,
        position: gpui::Point<Pixels>,
        cx: &mut Context<Self>,
    ) {
        self.end_pointer_drag(cx);
        let in_plot = self
            .graph_screen_position(position)
            .is_some_and(|screen| (0. ..=1.).contains(&screen[0]));
        if !in_plot {
            self.graph_interaction = GraphInteraction::Idle;
            return;
        }
        self.graph_interaction = GraphInteraction::Background {
            origin: position,
            dragged: false,
        };
        if self.transport.read(cx).is_playing() {
            self.transport.update(cx, |transport, cx| {
                transport.begin_scrub(ScrubSource::AnimationCurve, cx);
            });
        }
        if let Some(frame) = self.frame_at_graph_position(position, cx) {
            self.transport
                .update(cx, |transport, cx| transport.set_playhead(frame, cx));
        }
    }

    /// Promote a press to a scrub after the drag threshold, then follow the pointer.
    pub(in crate::ui::panels::animation_curve) fn graph_press_moved(
        &mut self,
        event: &gpui::MouseMoveEvent,
        cx: &mut Context<Self>,
    ) {
        // Mouse-up can be lost outside the window. A hover must never continue
        // or promote a stale press into a scrub when the pointer returns.
        if event.pressed_button != Some(MouseButton::Left) {
            self.end_pointer_drag(cx);
            self.end_graph_press(cx);
            return;
        }
        let GraphInteraction::Background { origin, dragged } = self.graph_interaction else {
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
                dragged: true,
            };
            if !self
                .transport
                .read(cx)
                .is_scrubbing(ScrubSource::AnimationCurve)
            {
                self.transport.update(cx, |transport, cx| {
                    transport.begin_scrub(ScrubSource::AnimationCurve, cx);
                });
            }
        }
        if self
            .transport
            .read(cx)
            .is_scrubbing(ScrubSource::AnimationCurve)
        {
            self.seek_playhead_from_graph(position, cx);
        }
    }

    pub(in crate::ui::panels::animation_curve) fn end_graph_press(
        &mut self,
        cx: &mut Context<Self>,
    ) {
        if let GraphInteraction::Background { .. } = self.graph_interaction {
            self.graph_interaction = GraphInteraction::Idle;
        }
        self.finish_playhead_scrub(cx);
    }

    pub(in crate::ui::panels::animation_curve) fn frame_at_graph_position(
        &self,
        position: gpui::Point<Pixels>,
        cx: &App,
    ) -> Option<Frame> {
        let screen = self.graph_screen_position(position)?;
        let progress = screen[0].clamp(0., 1.);
        let selected = self.selected_curve(cx)?;
        selected.frame_at_pattern_frame(selected.segment_frame_at_progress(progress))
    }

    pub(in crate::ui::panels::animation_curve) fn time_at_graph_position(
        &self,
        position: gpui::Point<Pixels>,
        cx: &App,
    ) -> Option<TimelineTime> {
        let progress = self.graph_screen_position(position)?[0].clamp(0., 1.);
        let selected = self.selected_curve(cx)?;
        Some(
            selected
                .clock
                .key_time_at(selected.segment_frame_at_progress(progress)),
        )
    }

    pub(in crate::ui::panels::animation_curve) fn time_at_overview_position(
        &self,
        position: gpui::Point<Pixels>,
        cx: &App,
    ) -> Option<TimelineTime> {
        let progress = self.graph_screen_position(position)?[0].clamp(0., 1.);
        let selected = self.selected_curve(cx)?;
        let frame = selected.clock.frame_at_progress(f64::from(progress));
        Some(selected.clock.key_time_at(frame))
    }

    pub(in crate::ui::panels::animation_curve) fn seek_playhead_from_graph(
        &mut self,
        position: gpui::Point<Pixels>,
        cx: &mut Context<Self>,
    ) {
        if let Some(frame) = self.frame_at_graph_position(position, cx) {
            self.transport
                .update(cx, |transport, cx| transport.set_playhead(frame, cx));
        }
    }

    pub(in crate::ui::panels::animation_curve) fn finish_playhead_scrub(
        &mut self,
        cx: &mut Context<Self>,
    ) {
        self.transport.update(cx, |transport, cx| {
            transport.end_scrub(ScrubSource::AnimationCurve, cx);
        });
    }
}

pub(in crate::ui::panels::animation_curve) fn point_layout(
    position: [f32; 2],
    radius: f32,
) -> ([f32; 2], f32, f32) {
    let screen = position.map(|value| {
        if value.abs() <= AnimationCurveEditor::SCREEN_EDGE_EPSILON {
            0.
        } else if (value - 1.).abs() <= AnimationCurveEditor::SCREEN_EDGE_EPSILON {
            1.
        } else {
            value
        }
    });
    (
        screen,
        AnimationCurveEditor::GRAPH_INSET_LEFT * (1. - screen[0])
            - AnimationCurveEditor::GRAPH_INSET_RIGHT * screen[0]
            - radius,
        AnimationCurveEditor::GRAPH_INSET_BOTTOM * (1. - screen[1])
            - AnimationCurveEditor::GRAPH_INSET_TOP * screen[1]
            - radius,
    )
}

pub(in crate::ui::panels::animation_curve) fn screen_position_is_visible(screen: [f32; 2]) -> bool {
    let visible =
        -AnimationCurveEditor::SCREEN_EDGE_EPSILON..=1. + AnimationCurveEditor::SCREEN_EDGE_EPSILON;
    visible.contains(&screen[0]) && visible.contains(&screen[1])
}
