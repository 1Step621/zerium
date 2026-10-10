use super::*;
use crate::ui::number_input::NumberValueDrag;

#[derive(Clone)]
pub(super) struct PropertyValueDragOrigin {
    pub(super) target: PropertyAddress,
    pub(super) input_id: ControlId,
    pub(super) adjustment: crate::ui::numeric_property::NumericDrag,
    pub(super) min: f64,
    pub(super) max: f64,
}

impl PropertyInspector {
    pub(super) fn begin_number_drag(
        &mut self,
        origin: Rc<PropertyValueDragOrigin>,
        cx: &mut Context<Self>,
    ) {
        self.editor
            .update(cx, |editor, _| editor.finish_history_group());
        self.store.number_drag_origin = Some(origin);
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
        let Some(origin) = self
            .store
            .number_drag_origin
            .as_ref()
            .filter(|origin| {
                self.store
                    .text_inputs
                    .get(&origin.input_id)
                    .is_some_and(|state| state.input.entity_id() == drag.input_id)
            })
            .cloned()
        else {
            return;
        };
        if !self.focus_handle.is_focused(window) {
            self.focus_handle.focus(window, cx);
        }
        cx.set_active_drag_cursor_style(CursorStyle::ResizeLeftRight, window);
        let pointer_x = f32::from(event.event.position.x);
        let fine = event.event.modifiers.shift;
        self.handle_value_drag(&origin, pointer_x, fine, window, cx);
    }

    pub(super) fn finish_number_drag(&mut self, cx: &mut Context<Self>) {
        self.store.number_drag_origin = None;
        self.editor
            .update(cx, |editor, _| editor.finish_history_group());
    }
}
