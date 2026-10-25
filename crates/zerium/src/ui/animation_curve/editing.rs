use super::*;
use crate::{
    app::copy_buffer::CopyBuffer,
    ui::{TimelineEditorEntityExt as _, animation::AnimationSynchronization},
};
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
    pub(super) fn synchronization(
        &self,
        selected: &SelectedCurve,
        part: AnimationEditTarget,
        window: &Window,
        cx: &App,
    ) -> Option<AnimationSynchronization> {
        let editor = self.editor.read(cx);
        let active = match (&self.graph_interaction, part) {
            (GraphInteraction::StopDrag { stop, edit, .. }, AnimationEditTarget::Stop(index))
                if *stop == index =>
            {
                Some(edit.as_ref())
            }
            (GraphInteraction::HandleDrag { edit, .. }, AnimationEditTarget::Segment(index))
                if index == selected.source_segment =>
            {
                Some(edit.as_ref())
            }
            (_, AnimationEditTarget::Segment(index)) if index == selected.source_segment => self
                .step_input
                .as_ref()
                .and_then(|input| input.edit_for(selected)),
            _ => None,
        };
        let preview;
        let edit = if let Some(edit) = active {
            if !edit.is_current(editor) {
                return None;
            }
            edit
        } else {
            preview =
                editor.begin_animation_edit(&selected.address, part, !window.modifiers().alt)?;
            &preview
        };
        Some(AnimationSynchronization::from_edit(&self.editor, edit))
    }

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

    pub(super) fn prepare_stop_drag(
        &mut self,
        stop: usize,
        synchronize: bool,
        cx: &mut Context<Self>,
    ) {
        let Some(selected) = self.selected_curve(cx) else {
            return;
        };
        let Some(edit) = self.editor.read(cx).begin_animation_edit(
            &selected.address,
            AnimationEditTarget::Stop(stop),
            synchronize,
        ) else {
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
        let GraphInteraction::StopDrag { edit, .. } = &mut self.graph_interaction else {
            return;
        };
        self.editor
            .update_if_changed(cx, |editor| editor.move_animation_stop(edit, time));
    }

    pub(super) fn prepare_handle_drag(
        &mut self,
        handle: BezierHandle,
        synchronize: bool,
        cx: &mut Context<Self>,
    ) {
        let Some(selected) = self.selected_curve(cx) else {
            return;
        };
        let Some(edit) = self.editor.read(cx).begin_animation_edit(
            &selected.address,
            AnimationEditTarget::Segment(selected.source_segment),
            synchronize,
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
            if matches!(drag.parameter, CurveNumberParameter::StepCount)
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
        event: &gpui::DragMoveEvent<crate::ui::number_input::NumberValueDrag>,
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
                self.set_step_count(&input, value, !event.event.modifiers.alt, cx);
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
        synchronize: bool,
        cx: &mut Context<Self>,
    ) {
        if !target.context.is_current(self.editor.read(cx)) {
            return;
        }
        let Some(mut edit) = self.editor.read(cx).begin_animation_edit(
            &target.address,
            AnimationEditTarget::Segment(segment),
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
            let editor = self.editor.read(cx);
            if !editor
                .property_schema(&selected.address)?
                .is_editable(selected.address.scalar_index)
            {
                return None;
            }
            Some((
                AnimationActionTarget::new(selected.address.clone(), editor),
                selected.source_segment,
                interpolation,
            ))
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
                    if let Some((target, segment, interpolation)) = &paste {
                        paste_editor.update(cx, |view, cx| {
                            view.set_interpolation(target, *segment, *interpolation, true, cx);
                        });
                    }
                },
            )
    }
}
