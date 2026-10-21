use super::*;
use crate::ui::input::set_input_text;
use crate::ui::number_input::NumberValueDrag;

pub(super) struct PropertyValueDrag {
    input_id: ControlId,
    adjustment: crate::ui::numeric_property::NumericDrag,
    min: f64,
    max: f64,
    edit: ValueEdit,
}

impl PropertyInspector {
    pub(super) fn prepare_value_drag(
        &mut self,
        target: &PropertyAddress,
        spec: &NumericInputSpec,
        input_id: &ControlId,
        event: &MouseDownEvent,
        cx: &mut Context<Self>,
    ) {
        if self.inspector_item_id(cx) != Some(target.item_id) {
            return;
        }
        let edit_target = match input_id {
            ControlId::AnimationStop { .. } => {
                let Some(stop) = self.store.tree.animation_stop(input_id) else {
                    return;
                };
                ValueEditTarget::Animation(stop.plan.clone())
            }
            ControlId::Property(_) => {
                if self.editor.read(cx).animation_track(target).is_some() {
                    return;
                }
                ValueEditTarget::Property(target.clone())
            }
            _ => return,
        };
        self.finish_number_drag(cx);
        let Some(edit) = self.editor.update(cx, |editor, _| {
            editor.finish_history_group();
            editor.begin_value_gesture(vec![edit_target])
        }) else {
            return;
        };
        let start_value = self
            .store
            .inputs
            .text_inputs
            .get(input_id)
            .and_then(|state| spec.parse_number(&state.input.read(cx).value()))
            .or_else(|| edit.value(self.editor.read(cx), 0)?.numeric_scalar())
            .unwrap_or(spec.min);
        self.store.number_drag = Some(PropertyValueDrag {
            input_id: input_id.clone(),
            adjustment: crate::ui::numeric_property::NumericDrag {
                start_x: f32::from(event.position.x),
                start_value,
                step: spec.step,
                sensitivity: spec.drag_sensitivity(),
            },
            min: spec.min,
            max: spec.max,
            edit,
        });
    }

    pub(super) fn handle_number_drag(
        &mut self,
        event: &DragMoveEvent<NumberValueDrag>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let drag = event.drag(cx);
        if drag.owner_id != cx.entity_id() {
            return;
        }
        let Some(input) = self
            .store
            .number_drag
            .as_ref()
            .and_then(|drag| self.store.inputs.text_inputs.get(&drag.input_id))
            .map(|state| state.input.clone())
            .filter(|input| input.entity_id() == drag.input_id)
        else {
            return;
        };
        if !self.focus_handle.is_focused(window) {
            self.focus_handle.focus(window, cx);
        }
        cx.set_active_drag_cursor_style(CursorStyle::ResizeLeftRight, window);
        let Some(drag) = &mut self.store.number_drag else {
            return;
        };
        let value = drag
            .adjustment
            .value_at(
                f32::from(event.event.position.x),
                event.event.modifiers.shift,
            )
            .clamp(drag.min, drag.max);
        let displayed = self.editor.update(cx, |editor, cx| {
            let updated = drag.edit.value(editor, 0)?.with_numeric_scalar(value)?;
            if editor.update_value_edit(&mut drag.edit, &[updated]) {
                cx.notify();
            }
            drag.edit.value(editor, 0)
        });
        if let Some(value) = displayed {
            set_input_text(&input, value.numeric_text().unwrap_or_default(), window, cx);
        }
    }

    pub(super) fn finish_number_drag(&mut self, cx: &mut Context<Self>) {
        self.finish_animation_value_edit();
        if self.store.number_drag.take().is_some() {
            self.editor
                .update(cx, |editor, _| editor.finish_history_group());
        }
    }
}
