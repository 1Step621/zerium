use super::*;
use crate::ui::TimelineEditorEntityExt as _;

impl AnimationCurveEditor {
    fn stop_location_at_time(&self, time: TimelineTime, cx: &App) -> Option<(SelectedCurve, f32)> {
        let selected = self.selected_curve(cx)?;
        let editor = self.editor.read(cx);
        let item = editor.item(selected.address.item_id)?;
        let track = item.animation_track(
            selected.address.effect_id,
            &selected.address.property_id,
            selected.address.element_id,
            selected.address.scalar_index,
        )?;
        let progress = item.animation_clock(track).pattern_progress_at(time);
        (progress > 0. && progress < 1. && track.stop_index_at(progress).is_none())
            .then_some((selected, progress))
    }

    pub(super) fn can_add_stop_at_time(&self, time: TimelineTime, cx: &App) -> bool {
        self.stop_location_at_time(time, cx).is_some()
    }

    pub(super) fn add_stop_at_time(&mut self, time: TimelineTime, cx: &mut Context<Self>) {
        let Some((selected, progress)) = self.stop_location_at_time(time, cx) else {
            return;
        };
        let Some(value) = (|| {
            let editor = self.editor.read(cx);
            let item = editor.item(selected.address.item_id)?;
            let track = item.animation_track(
                selected.address.effect_id,
                &selected.address.property_id,
                selected.address.element_id,
                selected.address.scalar_index,
            )?;
            track
                .stops()
                .iter()
                .min_by(|left, right| {
                    (left.position() - progress)
                        .abs()
                        .total_cmp(&(right.position() - progress).abs())
                })
                .map(|stop| stop.value().clone())
        })() else {
            return;
        };
        let frame = Self::frame_at_pattern_progress(&selected, progress);
        let target = selected.address;
        let changed = self.editor.update_if_changed(cx, |editor| {
            editor
                .insert_animation_stop(&target, progress, value)
                .is_some()
        });
        if !changed {
            return;
        }
        if let Some(frame) = frame {
            self.transport
                .update(cx, |transport, cx| transport.set_playhead(frame, cx));
        }
        cx.notify();
    }

    pub(super) fn move_point(
        &mut self,
        point: CurvePoint,
        position: gpui::Point<Pixels>,
        cx: &mut Context<Self>,
    ) {
        if !matches!(self.graph_interaction, GraphInteraction::HandleDrag { point: active } if active == point)
        {
            return;
        }
        match point {
            CurvePoint::HandleIn(index) => {
                self.move_handle(index, BezierHandle::In, position, cx);
            }
            CurvePoint::HandleOut(index) => {
                self.move_handle(index, BezierHandle::Out, position, cx);
            }
        }
    }

    pub(super) fn begin_stop_drag(
        &mut self,
        stop: usize,
        synchronize: bool,
        cx: &mut Context<Self>,
    ) {
        let Some(selected) = self.selected_curve(cx) else {
            return;
        };
        let Some(progress) = selected.source_stop_positions.get(stop).copied() else {
            return;
        };
        let Some(edit) = self.editor.read(cx).begin_animation_edit(
            &selected.address,
            AnimationEditTarget::Stop(stop),
            synchronize,
        ) else {
            return;
        };
        self.animation_edit = Some(edit);
        let time = Self::time_at_source_progress(&selected, progress);
        let snap_playhead = self.editor.read(cx).playhead();
        let follow_focus = (TimelineTime::from_frame(snap_playhead) == time)
            .then(|| self.selection.read(cx).focused_segment())
            .flatten()
            .filter(|segment| *segment == stop || segment.saturating_add(1) == stop);
        self.editor
            .update(cx, |editor, _| editor.finish_history_group());
        self.graph_interaction = GraphInteraction::StopDrag {
            stop,
            time,
            snap_playhead,
            follow_focus,
        };
        cx.notify();
    }

    pub(super) fn move_stop_from_overview(
        &mut self,
        drag: &StopPositionDrag,
        pointer_x: f32,
        snap_disabled: bool,
        cx: &mut Context<Self>,
    ) {
        let (snap_playhead, follow_focus) = match self.graph_interaction {
            GraphInteraction::StopDrag {
                stop,
                snap_playhead,
                follow_focus,
                ..
            } if stop == drag.stop => (snap_playhead, follow_focus),
            _ => return,
        };
        let Some(selected) = self.selected_curve(cx) else {
            return;
        };
        let Some(bounds) = self.graph_bounds else {
            return;
        };
        let Some(range) = self
            .animation_edit
            .as_ref()
            .and_then(|edit| edit.time_range())
        else {
            return;
        };
        let minimum = range.start().frames();
        let maximum = range.end().frames();
        let plot_left = f32::from(bounds.origin.x) + Self::GRAPH_INSET_LEFT;
        let plot_width =
            f32::from(bounds.size.width) - Self::GRAPH_INSET_LEFT - Self::GRAPH_INSET_RIGHT;
        if plot_width <= 0. {
            return;
        }
        let requested = ((pointer_x - plot_left) / plot_width).clamp(0., 1.);
        let requested_time = selected
            .clock
            .time_at(requested)
            .frames()
            .round()
            .clamp(minimum, maximum);
        let mut time = TimelineTime::from_frames(requested_time);
        if !snap_disabled {
            let editor = self.editor.read(cx);
            let frame_rate = editor.frame_rate();
            let seconds = time.seconds(frame_rate);
            let pixels_per_second = f64::from(plot_width) * frame_rate.frames_per_second()
                / selected.clock.span_frames();
            let offset = time_grid::snap_offset_seconds(
                &[seconds],
                editor,
                snap_playhead,
                [],
                pixels_per_second,
            );
            time = TimelineTime::from_frames(
                ((seconds + offset) * frame_rate.frames_per_second())
                    .round()
                    .clamp(minimum, maximum),
            );
        }
        let Some(edit) = &mut self.animation_edit else {
            return;
        };
        let changed = self.editor.update_if_changed(cx, |editor| {
            editor.move_animation_stop(edit, time).is_some()
        });
        self.graph_interaction = GraphInteraction::StopDrag {
            stop: drag.stop,
            time,
            snap_playhead,
            follow_focus,
        };
        if changed {
            if let Some(segment) = follow_focus {
                self.selection.read(cx).focus_segment(segment);
                self.transport.update(cx, |transport, cx| {
                    transport.set_playhead(time.nearest_frame(), cx);
                });
            }
            cx.notify();
        }
    }

    pub(super) fn end_pointer_drag(&mut self, cx: &mut Context<Self>) {
        if let Some(inputs) = &mut self.repeat_inputs {
            inputs.drag = None;
        }
        if matches!(
            self.graph_interaction,
            GraphInteraction::HandleDrag { .. } | GraphInteraction::StopDrag { .. }
        ) {
            self.graph_interaction = GraphInteraction::Idle;
            self.animation_edit = None;
            self.handle_drag_view = None;
            cx.notify();
        }
    }

    pub(super) fn graph_pixels_per_second(&self, duration_seconds: f32) -> f64 {
        let plot_width = self.graph_bounds.map_or(220., |bounds| {
            f32::from(bounds.size.width - px(Self::GRAPH_INSET_LEFT + Self::GRAPH_INSET_RIGHT))
                .max(1.)
        });
        f64::from(plot_width / duration_seconds.max(f32::EPSILON))
    }

    pub(super) fn move_handle(
        &mut self,
        index: usize,
        handle: BezierHandle,
        position: gpui::Point<Pixels>,
        cx: &mut Context<Self>,
    ) {
        let Some(selected) = self.selected_curve(cx) else {
            return;
        };
        let Some(position) = self.graph_screen_position(position) else {
            return;
        };
        let Some((_, position)) = selected
            .curve
            .local_handle_position(index, handle, position)
        else {
            return;
        };
        let Some(SegmentInterpolation::Custom(mut curve)) =
            selected.curve.interpolations.first().copied()
        else {
            return;
        };
        if !curve.set_handle(handle, position) {
            return;
        }
        let Some(edit) = &mut self.animation_edit else {
            return;
        };
        self.editor.update_if_changed(cx, |editor| {
            editor.set_animation_interpolation(edit, SegmentInterpolation::Custom(curve))
        });
    }

    pub(super) fn remove_source_stop(&mut self, source_stop: usize, cx: &mut Context<Self>) {
        let Some(selected) = self.selected_curve(cx) else {
            return;
        };
        let target = selected.address;
        self.editor.update_if_changed(cx, |editor| {
            editor.remove_animation_stop(&target, source_stop)
        });
    }

    pub(super) fn set_interpolation(
        &mut self,
        interpolation: SegmentInterpolation,
        synchronize: bool,
        cx: &mut Context<Self>,
    ) {
        let Some(selected) = self.selected_curve(cx) else {
            return;
        };
        let Some(mut edit) = self.editor.read(cx).begin_animation_edit(
            &selected.address,
            AnimationEditTarget::Segment(selected.source_segment),
            synchronize,
        ) else {
            return;
        };
        self.editor.update_if_changed(cx, |editor| {
            editor.set_animation_interpolation(&mut edit, interpolation)
        });
    }
}
