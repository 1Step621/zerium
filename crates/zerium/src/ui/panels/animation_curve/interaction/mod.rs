pub(super) mod navigation;
use super::*;
use crate::{app::copy_buffer::CopyBuffer, ui::TimelineEditorEntityExt as _};
use ::ui::menu::PopupMenu;
use rust_i18n::t;
use zerium_core::timeline::EditContext;

/// Menu commands retain their original property and authored document state.
#[derive(Clone)]
pub(super) struct AnimationActionTarget {
    address: PropertyAddress,
    context: EditContext,
}

impl AnimationActionTarget {
    pub(super) fn new(address: PropertyAddress, editor: &TimelineEditor) -> Self {
        Self {
            address,
            context: editor.edit_context(),
        }
    }
}

impl AnimationCurveEditor {
    pub(super) fn discard_stale_edits(&mut self, cx: &mut Context<Self>) {
        let editor = self.editor.read(cx);
        match &self.graph_interaction {
            GraphInteraction::HandleDrag { edit, .. } | GraphInteraction::StopDrag { edit, .. }
                if !edit.is_current(editor) =>
            {
                self.graph_interaction = GraphInteraction::Idle;
            }
            _ => {}
        }
        if self
            .step_input
            .as_mut()
            .is_some_and(|input| input.discard_stale_edit(editor))
        {
            self.number_drag = None;
        }
    }

    pub(super) fn can_add_stop_at_time(
        &self,
        target: &AnimationActionTarget,
        time: TimelineTime,
        cx: &App,
    ) -> bool {
        let editor = self.editor.read(cx);
        target.context.is_current(editor)
            && editor
                .animation_stop_insertion_frame(&target.address, time)
                .is_some()
    }

    pub(super) fn add_stop_at_time(
        &mut self,
        target: &AnimationActionTarget,
        time: TimelineTime,
        cx: &mut Context<Self>,
    ) {
        let editor = self.editor.read(cx);
        if !target.context.is_current(editor) {
            return;
        }
        let Some(pattern_frame) = editor.animation_stop_insertion_frame(&target.address, time)
        else {
            return;
        };
        let Some(item) = editor.item(target.address.item_id) else {
            return;
        };
        let Some(track) = editor.animation_track(&target.address) else {
            return;
        };
        let frame = item
            .animation_clock(track)
            .first_occurrence(
                pattern_frame,
                TimelineTime::from_frame(item.start()),
                TimelineTime::from_frame(Frame::new(
                    item.end_exclusive(editor.frame_rate())
                        .get()
                        .saturating_sub(1),
                )),
            )
            .map(TimelineTime::nearest_frame);
        let changed = self.editor.update_if_changed(cx, |editor| {
            editor
                .insert_animation_stop_at_time(&target.address, time)
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

    pub(super) fn prepare_stop_drag(&mut self, stop: usize, cx: &mut Context<Self>) {
        let Some(selected) = self.selected_curve(cx) else {
            return;
        };
        let Some(edit) = self
            .editor
            .read(cx)
            .begin_animation_edit(&selected.address, AnimationEditTarget::Stop(stop))
        else {
            return;
        };
        let snap_playhead = self.editor.read(cx).playhead();
        self.end_pointer_drag(cx);
        self.editor
            .update(cx, |editor, _| editor.finish_history_group());
        self.graph_interaction = GraphInteraction::StopDrag {
            stop,
            snap_playhead,
            edit: Box::new(edit),
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
        let (snap_playhead, range) = match &self.graph_interaction {
            GraphInteraction::StopDrag {
                stop,
                snap_playhead,
                edit,
                ..
            } if *stop == drag.stop => (*snap_playhead, edit.time_range()),
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
        let Some(geometry) = self.graph_geometry() else {
            return;
        };
        let plot_width = f32::from(geometry.plot.size.width);
        if plot_width <= 0. {
            return;
        }
        let requested = geometry.progress_at_x(pointer_x).clamp(0., 1.);
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
        let GraphInteraction::StopDrag { edit, .. } = &mut self.graph_interaction else {
            return;
        };
        self.editor
            .update_if_changed(cx, |editor| editor.move_animation_stop(edit, time));
    }

    pub(super) fn prepare_handle_drag(&mut self, handle: BezierHandle, cx: &mut Context<Self>) {
        let Some(selected) = self.selected_curve(cx) else {
            return;
        };
        let Some(edit) = self.editor.read(cx).begin_animation_edit(
            &selected.address,
            AnimationEditTarget::Segment(selected.source_segment),
        ) else {
            return;
        };
        self.end_pointer_drag(cx);
        self.graph_interaction = GraphInteraction::HandleDrag {
            handle,
            edit: Box::new(edit),
            view: Box::new(selected),
        };
        self.finish_playhead_scrub(cx);
        self.editor
            .update(cx, |editor, _| editor.finish_history_group());
        cx.notify();
    }

    pub(super) fn finish_history_drag(&mut self, cx: &mut Context<Self>) {
        if self
            .step_input
            .as_mut()
            .is_some_and(|input| input.finish_edit())
        {
            cx.notify();
        }
        self.editor
            .update(cx, |editor, _| editor.finish_history_group());
    }

    pub(super) fn end_pointer_drag(&mut self, cx: &mut Context<Self>) {
        if let Some(drag) = self.number_drag.take() {
            if let CurveNumberParameter::StepCount = drag.parameter
                && let Some(input) = &mut self.step_input
            {
                input.finish_edit();
            }
            cx.notify();
        }
        if let GraphInteraction::HandleDrag { .. } | GraphInteraction::StopDrag { .. } =
            self.graph_interaction
        {
            self.graph_interaction = GraphInteraction::Idle;
            cx.notify();
        }
    }

    pub(super) fn move_number(
        &mut self,
        event: &gpui::DragMoveEvent<crate::ui::components::input::number::NumberValueDrag>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let target = event.drag(cx);
        let Some(drag) = &self.number_drag else {
            return;
        };
        if target.owner_id != cx.entity_id() || target.input_id != drag.input.entity_id() {
            return;
        }
        if event.event.pressed_button != Some(MouseButton::Left) {
            self.end_pointer_drag(cx);
            return;
        }
        let parameter = drag.parameter;
        let input = drag.input.clone();
        let value = drag.motion.value_at(
            f32::from(event.event.position.x),
            event.event.modifiers.shift,
        );
        self.focus_handle.focus(window, cx);
        cx.set_active_drag_cursor_style(gpui::CursorStyle::ResizeLeftRight, window);
        match parameter {
            CurveNumberParameter::StepCount => {
                self.set_step_count(&input, value, cx);
                self.sync_step_text(&input, window, cx);
            }
            CurveNumberParameter::Repeat(parameter) => {
                if let Some(inputs) = self
                    .repeat_inputs
                    .as_ref()
                    .filter(|inputs| inputs.fields[parameter.index()].input == input)
                {
                    let address = inputs.address.clone();
                    self.set_repeat_number(&address, parameter, value, cx);
                }
            }
        }
    }

    pub(super) fn graph_pixels_per_second(&self, duration_seconds: f32) -> f64 {
        let plot_width = self
            .graph_geometry()
            .map_or(220., |geometry| f32::from(geometry.plot.size.width).max(1.));
        f64::from(plot_width / duration_seconds.max(f32::EPSILON))
    }

    pub(super) fn move_handle(
        &mut self,
        handle: BezierHandle,
        position: gpui::Point<Pixels>,
        cx: &mut Context<Self>,
    ) {
        let GraphInteraction::HandleDrag { handle: active, .. } = &self.graph_interaction else {
            return;
        };
        if *active != handle {
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

    pub(super) fn remove_source_stop(
        &mut self,
        target: &AnimationActionTarget,
        source_stop: usize,
        cx: &mut Context<Self>,
    ) {
        self.editor.update_if_changed(cx, |editor| {
            target.context.is_current(editor)
                && editor.remove_animation_stop(&target.address, source_stop)
        });
    }

    pub(super) fn set_interpolation(
        &mut self,
        target: &AnimationActionTarget,
        segment: usize,
        interpolation: SegmentInterpolation,
        cx: &mut Context<Self>,
    ) {
        if !target.context.is_current(self.editor.read(cx)) {
            return;
        }
        let Some(mut edit) = self
            .editor
            .read(cx)
            .begin_animation_edit(&target.address, AnimationEditTarget::Segment(segment))
        else {
            return;
        };
        self.editor.update_if_changed(cx, |editor| {
            editor.set_animation_interpolation(&mut edit, interpolation)
        });
    }

    pub(super) fn graph_menu(
        &self,
        menu: PopupMenu,
        position: gpui::Point<Pixels>,
        cx: &Context<Self>,
    ) -> PopupMenu {
        let selected = self.selected_curve(cx);
        let menu = if let Some(selected) = &selected {
            self.stop_menu(
                menu,
                selected,
                self.graph_stop_at_position(position, selected),
                self.time_at_graph_position(position, selected),
                cx,
            )
        } else {
            menu
        };
        self.curve_menu(menu, selected.as_ref(), cx)
    }

    pub(super) fn overview_menu(
        &self,
        menu: PopupMenu,
        position: gpui::Point<Pixels>,
        cx: &Context<Self>,
    ) -> PopupMenu {
        let menu = menu.action_context(self.focus_handle.clone());
        let menu = if let Some(selected) = self.selected_curve(cx) {
            self.stop_menu(
                menu,
                &selected,
                self.overview_stop_at_position(position, &selected),
                self.time_at_overview_position(position, &selected),
                cx,
            )
        } else {
            menu
        };
        self.animation_timing_menu(menu, cx)
    }

    /// Key editing is always the first group, followed by clipboard commands.
    pub(super) fn stop_menu(
        &self,
        menu: PopupMenu,
        selected: &SelectedCurve,
        stop: Option<usize>,
        time: Option<TimelineTime>,
        cx: &Context<Self>,
    ) -> PopupMenu {
        let target = AnimationActionTarget::new(selected.address.clone(), self.editor.read(cx));
        let view = cx.entity();
        let menu = if let Some(stop) = stop {
            menu.menu_handler_with_icon_and_disabled(
                t!("curve.delete_stop").to_string(),
                IconName::Delete,
                stop == 0 || stop + 1 == selected.source_stop_frames.len(),
                move |_, cx| {
                    view.update(cx, |view, cx| view.remove_source_stop(&target, stop, cx));
                },
            )
        } else if let Some(time) = time {
            if self.can_add_stop_at_time(&target, time, cx) {
                menu.menu_handler_with_icon(
                    t!("curve.add_stop").to_string(),
                    IconName::Plus,
                    move |_, cx| {
                        view.update(cx, |view, cx| view.add_stop_at_time(&target, time, cx));
                    },
                )
            } else {
                menu.item(PopupMenuItem::Label(
                    t!("curve.stop_add_error").to_string().into(),
                ))
            }
        } else {
            return menu;
        };
        menu.separator()
    }

    fn curve_menu(
        &self,
        menu: PopupMenu,
        selected: Option<&SelectedCurve>,
        cx: &Context<Self>,
    ) -> PopupMenu {
        let action_context = self.focus_handle.clone();
        let interpolation = selected.map(|selected| selected.curve.interpolation);
        let copied = cx
            .try_global::<CopyBuffer>()
            .and_then(|buffer| buffer.curve);
        let paste = copied.and_then(|interpolation| {
            let selected = selected?;
            let editor = self.editor.read(cx);
            editor.property_schema(&selected.address)?;
            Some((
                AnimationActionTarget::new(selected.address.clone(), editor),
                selected.source_segment,
                interpolation,
            ))
        });
        let paste_editor = cx.entity();
        let menu = menu
            .action_context(action_context)
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
                    if let Some((target, segment, interpolation)) = &paste {
                        paste_editor.update(cx, |view, cx| {
                            view.set_interpolation(target, *segment, *interpolation, cx);
                        });
                    }
                },
            );
        self.animation_timing_menu(menu.separator(), cx)
    }

    pub(super) fn animation_timing_menu(&self, menu: PopupMenu, cx: &Context<Self>) -> PopupMenu {
        let editor = self.editor.read(cx);
        let address = self.selection.read(cx).address();
        let target = address.map(|address| AnimationActionTarget::new(address.clone(), editor));
        let paste_editor = cx.entity();
        crate::ui::components::animation::animation_timing_menu(
            menu,
            editor,
            address.cloned(),
            move |timing, cx| {
                if let Some(target) = &target {
                    paste_editor.update(cx, |view, cx| {
                        view.editor.update_if_changed(cx, |editor| {
                            target.context.is_current(editor)
                                && editor.paste_property_animation_timing(&target.address, timing)
                        });
                    });
                }
            },
            cx,
        )
    }
}
