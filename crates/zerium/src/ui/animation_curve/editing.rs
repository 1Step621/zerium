use super::*;

impl AnimationCurveEditor {
    fn stop_location_at_frame(&self, frame: Frame, cx: &App) -> Option<(SelectedCurve, f32)> {
        let selected = self.selected_curve(cx)?;
        let editor = self.editor.read(cx);
        let item = editor.item(selected.address.item_id)?;
        let progress = item.animation_progress_at_time(TimelineTime::from_frame(frame));
        let track = item.animation_track(
            selected.address.effect_id,
            &selected.address.property_id,
            selected.address.element_id,
            selected.address.scalar_index,
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
        let target = selected.address;
        let changed = self.editor.update(cx, |editor, cx| {
            let changed = editor
                .insert_animation_stop(&target, progress, value)
                .is_some();
            if changed {
                cx.notify();
            }
            changed
        });
        if !changed {
            return;
        }
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
        let Some(range) = self
            .animation_edit
            .as_ref()
            .and_then(|edit| edit.frame_range())
        else {
            return;
        };
        let minimum = range.start().get();
        let maximum = range.end().get();
        let plot_left = f32::from(bounds.origin.x) + Self::GRAPH_INSET_LEFT;
        let plot_width =
            f32::from(bounds.size.width) - Self::GRAPH_INSET_LEFT - Self::GRAPH_INSET_RIGHT;
        if plot_width <= 0. {
            return;
        }
        let requested = ((pointer_x - plot_left) / plot_width).clamp(0., 1.);
        let span_frames = selected.clip_duration.get().saturating_sub(1).max(1);
        let clip_start = selected.clip_start.get();
        let requested_frame = clip_start
            .saturating_add((f64::from(requested) * span_frames as f64).round() as u64)
            .clamp(minimum, maximum);
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
        let Some(edit) = &mut self.animation_edit else {
            return;
        };
        let changed = self.editor.update(cx, |editor, cx| {
            let changed = editor
                .move_animation_stop(edit, Frame::new(frame))
                .is_some();
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

    pub(super) fn end_pointer_drag(&mut self, cx: &mut Context<Self>) {
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
        self.editor.update(cx, |editor, cx| {
            if editor.set_animation_interpolation(edit, SegmentInterpolation::Custom(curve)) {
                cx.notify();
            }
        });
    }

    pub(super) fn remove_source_stop(&mut self, source_stop: usize, cx: &mut Context<Self>) {
        let Some(selected) = self.selected_curve(cx) else {
            return;
        };
        let target = selected.address;
        self.editor.update(cx, |editor, cx| {
            if editor.remove_animation_stop(&target, source_stop) {
                cx.notify();
            }
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
        self.editor.update(cx, |editor, cx| {
            if editor.set_animation_interpolation(&mut edit, interpolation) {
                cx.notify();
            }
        });
    }
}
