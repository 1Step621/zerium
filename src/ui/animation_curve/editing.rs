use super::*;

impl AnimationCurveEditor {
    fn stop_location_at_frame(&self, frame: Frame, cx: &App) -> Option<(SelectedCurve, f32)> {
        let selected = self.selected_curve(cx)?;
        let editor = self.editor.read(cx);
        let item = editor.item(selected.target.item_id)?;
        let progress = item.animation_progress_at_time(TimelineTime::from_frame(frame));
        let track = item.animation_track(
            selected.target.effect_id,
            &selected.target.property_id,
            selected.target.element_id,
            selected.target.scalar_index,
        )?;
        (progress > 0. && progress < 1. && track.stop_index_at(progress).is_none())
            .then_some((selected, progress))
    }

    pub(super) fn can_add_stop_at_frame(&self, frame: Frame, cx: &App) -> bool {
        self.stop_location_at_frame(frame, cx).is_some()
    }

    pub(super) fn add_stop_at_frame(&mut self, frame: Frame, cx: &mut Context<Self>) {
        let Some((selected, progress)) = self.stop_location_at_frame(frame, cx) else {
            return;
        };
        let Some(value) = (|| {
            let editor = self.editor.read(cx);
            let item = editor.item(selected.target.item_id)?;
            let track = item.animation_track(
                selected.target.effect_id,
                &selected.target.property_id,
                selected.target.element_id,
                selected.target.scalar_index,
            )?;
            let index = track.stop_index_nearest(progress)?;
            track.stops().get(index).map(|stop| stop.value()).cloned()
        })() else {
            return;
        };
        let target = selected.target;
        let changed = self.editor.update(cx, |editor, cx| {
            let changed = editor
                .insert_selected_property_animation_stop(
                    target.effect_id,
                    target.property_id,
                    target.element_id,
                    target.scalar_index,
                    progress,
                    value,
                )
                .is_some();
            if changed {
                cx.notify();
            }
            changed
        });
        if !changed {
            return;
        }
        self.selected_segment = None;
        self.transport
            .update(cx, |transport, cx| transport.set_playhead(frame, cx));
        cx.notify();
    }

    pub(super) fn move_point(
        &mut self,
        point: CurvePoint,
        position: gpui::Point<Pixels>,
        cx: &mut Context<Self>,
    ) {
        self.graph_interaction = GraphInteraction::HandleDrag { point };
        match point {
            CurvePoint::HandleIn(index) => {
                self.move_handle(index, BezierHandle::In, position, cx);
            }
            CurvePoint::HandleOut(index) => {
                self.move_handle(index, BezierHandle::Out, position, cx);
            }
        }
    }

    pub(super) fn begin_stop_drag(&mut self, stop: usize, cx: &mut Context<Self>) {
        let Some(selected) = self.selected_curve(cx) else {
            return;
        };
        let Some(progress) = selected.source_stop_positions.get(stop).copied() else {
            return;
        };
        let frame = Self::frame_at_source_progress(&selected, progress);
        let snap_frame = self.editor.read(cx).playhead();
        let follow_focus = (snap_frame == frame)
            .then(|| self.selection.read(cx).focused_segment())
            .flatten()
            .filter(|segment| *segment == stop || segment.saturating_add(1) == stop);
        self.editor
            .update(cx, |editor, _| editor.finish_history_group());
        self.graph_interaction = GraphInteraction::StopDrag {
            stop,
            frame,
            snap_frame,
            follow_focus,
        };
        cx.notify();
    }

    pub(super) fn move_stop_from_overview(
        &mut self,
        drag: &StopPositionDrag,
        pointer_x: f32,
        cx: &mut Context<Self>,
    ) {
        let (snap_frame, follow_focus) = match self.graph_interaction {
            GraphInteraction::StopDrag {
                stop,
                snap_frame,
                follow_focus,
                ..
            } if stop == drag.stop => (snap_frame, follow_focus),
            _ => return,
        };
        let Some(selected) = self.selected_curve(cx) else {
            return;
        };
        let Some(bounds) = self.graph_bounds else {
            return;
        };
        let Some(previous) = drag
            .stop
            .checked_sub(1)
            .and_then(|index| selected.source_stop_positions.get(index))
            .copied()
        else {
            return;
        };
        let Some(next) = selected.source_stop_positions.get(drag.stop + 1).copied() else {
            return;
        };
        let plot_left = f32::from(bounds.origin.x) + Self::GRAPH_INSET_LEFT;
        let plot_width =
            f32::from(bounds.size.width) - Self::GRAPH_INSET_LEFT - Self::GRAPH_INSET_RIGHT;
        if plot_width <= 0. {
            return;
        }
        let requested = ((pointer_x - plot_left) / plot_width).clamp(0., 1.);
        let span_frames = selected.clip_duration.get().saturating_sub(1).max(1);
        let clip_start = selected.clip_start.get();
        let frame_for = |progress: f32| {
            clip_start.saturating_add((f64::from(progress) * span_frames as f64).round() as u64)
        };
        let minimum = frame_for(previous).saturating_add(1);
        let maximum = frame_for(next).saturating_sub(1);
        if minimum > maximum {
            return;
        }
        let requested_frame = frame_for(requested).clamp(minimum, maximum);
        let snap_frame_value = snap_frame.get();
        let snap_x = plot_left
            + plot_width
                * (snap_frame_value.saturating_sub(clip_start) as f32 / span_frames as f32);
        let frame = if (minimum..=maximum).contains(&snap_frame_value)
            && (pointer_x - snap_x).abs() <= Self::OVERVIEW_SNAP_DISTANCE
        {
            snap_frame_value
        } else {
            requested_frame
        };
        let progress = (frame.saturating_sub(clip_start) as f64 / span_frames as f64) as f32;
        let target = selected.target;
        let changed = self.editor.update(cx, |editor, cx| {
            let changed = editor.move_selected_animation_stop(
                target.effect_id,
                target.property_id,
                target.element_id,
                target.scalar_index,
                drag.stop,
                progress,
            );
            if changed {
                cx.notify();
            }
            changed
        });
        self.graph_interaction = GraphInteraction::StopDrag {
            stop: drag.stop,
            frame: Frame::new(frame),
            snap_frame,
            follow_focus,
        };
        if changed {
            if let Some(segment) = follow_focus {
                self.selection.read(cx).focus_segment(segment);
                self.transport.update(cx, |transport, cx| {
                    transport.set_playhead(Frame::new(frame), cx);
                });
            }
            cx.notify();
        }
    }

    pub(super) fn end_pointer_drag(&mut self) {
        if matches!(
            self.graph_interaction,
            GraphInteraction::HandleDrag { .. } | GraphInteraction::StopDrag { .. }
        ) {
            self.graph_interaction = GraphInteraction::Idle;
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
        let Some(position) = self.normalized_position(position) else {
            return;
        };
        let Some((_, position)) = selected
            .curve
            .local_handle_position(index, handle, position)
        else {
            return;
        };
        let target = selected.target;
        self.editor.update(cx, |editor, cx| {
            if editor.set_selected_animation_handle(
                target.effect_id,
                target.property_id,
                target.element_id,
                target.scalar_index,
                selected.source_segment,
                handle,
                position,
            ) {
                cx.notify();
            }
        });
    }

    pub(super) fn remove_source_stop(&mut self, source_stop: usize, cx: &mut Context<Self>) {
        let Some(selected) = self.selected_curve(cx) else {
            return;
        };
        let target = selected.target;
        self.editor.update(cx, |editor, cx| {
            if editor.remove_selected_animation_stop(
                target.effect_id,
                target.property_id,
                target.element_id,
                target.scalar_index,
                source_stop,
            ) {
                cx.notify();
            }
        });
        self.selected_segment = None;
    }

    pub(super) fn set_interpolation(
        &mut self,
        interpolation: SegmentInterpolation,
        cx: &mut Context<Self>,
    ) {
        let Some(selected) = self.selected_curve(cx) else {
            return;
        };
        let target = selected.target;
        self.editor.update(cx, |editor, cx| {
            if editor.set_selected_animation_interpolation(
                target.effect_id,
                target.property_id,
                target.element_id,
                target.scalar_index,
                selected.source_segment,
                interpolation,
            ) {
                cx.notify();
            }
        });
    }
}
