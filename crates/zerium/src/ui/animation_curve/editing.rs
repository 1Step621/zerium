use super::*;
use crate::{app::copy_buffer::CopyBuffer, ui::TimelineEditorEntityExt as _};
use ::ui::menu::PopupMenu;
use rust_i18n::t;
use std::cell::RefCell;

impl AnimationCurveEditor {
    pub(super) fn can_add_stop_at_time(&self, time: TimelineTime, cx: &App) -> bool {
        self.selection.read(cx).address().is_some_and(|address| {
            self.editor
                .read(cx)
                .animation_stop_insertion_frame(address, time)
                .is_some()
        })
    }

    pub(super) fn add_stop_at_time(&mut self, time: TimelineTime, cx: &mut Context<Self>) {
        let Some(selected) = self.selected_curve(cx) else {
            return;
        };
        let Some(pattern_frame) = self
            .editor
            .read(cx)
            .animation_stop_insertion_frame(&selected.address, time)
        else {
            return;
        };
        let frame = Self::frame_at_pattern_frame(&selected, pattern_frame);
        let target = selected.address;
        let changed = self.editor.update_if_changed(cx, |editor| {
            editor
                .insert_animation_stop_at_time(&target, time)
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

    pub(super) fn begin_stop_drag(
        &mut self,
        stop: usize,
        synchronize: bool,
        cx: &mut Context<Self>,
    ) {
        let Some(selected) = self.selected_curve(cx) else {
            return;
        };
        let Some(frame) = selected.source_stop_frames.get(stop).copied() else {
            return;
        };
        let Some(edit) = self.editor.read(cx).begin_animation_edit(
            &selected.address,
            AnimationEditTarget::Stop(stop),
            synchronize,
        ) else {
            return;
        };
        let time = selected.clock.key_time_at(frame);
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
            edit,
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
        let (snap_playhead, follow_focus, range) = match &self.graph_interaction {
            GraphInteraction::StopDrag {
                stop,
                snap_playhead,
                follow_focus,
                edit,
                ..
            } if *stop == drag.stop => (*snap_playhead, *follow_focus, edit.time_range()),
            _ => return,
        };
        let Some(range) = range else {
            return;
        };
        let minimum = range.start().frames();
        let maximum = range.end().frames();
        let Some(selected) = self.selected_curve(cx) else {
            return;
        };
        let Some(bounds) = self.graph_bounds else {
            return;
        };
        let plot_left = f32::from(bounds.origin.x) + Self::GRAPH_INSET_LEFT;
        let plot_width =
            f32::from(bounds.size.width) - Self::GRAPH_INSET_LEFT - Self::GRAPH_INSET_RIGHT;
        if plot_width <= 0. {
            return;
        }
        let requested = ((pointer_x - plot_left) / plot_width).clamp(0., 1.);
        let requested_time = selected
            .clock
            .key_time_at(selected.clock.frame_at_progress(f64::from(requested)))
            .frames()
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
        let GraphInteraction::StopDrag {
            edit,
            time: drag_time,
            ..
        } = &mut self.graph_interaction
        else {
            return;
        };
        let changed = self.editor.update_if_changed(cx, |editor| {
            editor.move_animation_stop(edit, time).is_some()
        });
        *drag_time = time;
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
        if let GraphInteraction::HandleDrag { .. } | GraphInteraction::StopDrag { .. } =
            self.graph_interaction
        {
            self.graph_interaction = GraphInteraction::Idle;
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
        handle: BezierHandle,
        position: gpui::Point<Pixels>,
        cx: &mut Context<Self>,
    ) {
        if !matches!(&self.graph_interaction, GraphInteraction::HandleDrag { handle: active, .. } if *active == handle)
        {
            return;
        }
        let Some(selected) = self.selected_curve(cx) else {
            return;
        };
        let Some(position) = self.graph_screen_position(position) else {
            return;
        };
        let Some(position) = selected.curve.local_handle_position(position) else {
            return;
        };
        let SegmentInterpolation::Custom(mut curve) = selected.curve.interpolation else {
            return;
        };
        if !curve.set_handle(handle, position) {
            return;
        }
        let GraphInteraction::HandleDrag { edit, .. } = &mut self.graph_interaction else {
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

    pub(super) fn curve_menu(&self, menu: PopupMenu, cx: &Context<Self>) -> PopupMenu {
        let action_context = self.focus_handle.clone();
        let selected = self.selected_curve(cx);
        let interpolation = selected
            .as_ref()
            .map(|selected| selected.curve.interpolation);
        let copied = cx
            .try_global::<CopyBuffer>()
            .and_then(|buffer| buffer.curve);
        let paste = copied.and_then(|interpolation| {
            let selected = selected.as_ref()?;
            let edit = self.editor.read(cx).begin_animation_edit(
                &selected.address,
                AnimationEditTarget::Segment(selected.source_segment),
                true,
            )?;
            Some((interpolation, RefCell::new(edit)))
        });
        let paste_editor = cx.entity();
        menu.action_context(action_context)
            .menu_handler_with_icon_and_disabled(
                t!("clipboard.copy_curve").to_string(),
                IconName::Copy,
                interpolation.is_none(),
                move |_, cx| {
                    if let Some(interpolation) = interpolation {
                        cx.default_global::<CopyBuffer>().curve = Some(interpolation);
                    }
                },
            )
            .menu_handler_with_icon_and_disabled(
                t!("clipboard.paste_curve").to_string(),
                IconName::PasteClipboard,
                paste.is_none(),
                move |_, cx| {
                    if let Some((interpolation, edit)) = &paste {
                        paste_editor.update(cx, |view, cx| {
                            view.editor.update_if_changed(cx, |editor| {
                                editor.set_animation_interpolation(
                                    &mut edit.borrow_mut(),
                                    *interpolation,
                                )
                            });
                        });
                    }
                },
            )
    }
}
