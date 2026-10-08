use super::*;
use crate::ui::TimelineEditorEntityExt as _;
use zerium_core::timeline::EditScope;

impl Preview {
    pub(super) fn begin_scalar_drag(
        &mut self,
        control: &PreviewScalarControl,
        composition_units_per_pixel: f32,
        event: &MouseDownEvent,
        cx: &mut Context<Self>,
    ) {
        if event.button != MouseButton::Left || composition_units_per_pixel <= 0. {
            return;
        }
        self.editor
            .update(cx, |editor, _| editor.finish_history_group());
        self.editor_drag = Some(PreviewScalarDragOrigin {
            control: control.clone(),
            pointer: [f32::from(event.position.x), f32::from(event.position.y)]
                [control.scalar.axis()],
            composition_units_per_pixel,
        });
        cx.notify();
    }

    pub(in crate::ui::preview) fn move_editor_control_from_pointer(
        &mut self,
        drag: &PreviewEditorDrag,
        pointer: [f32; 2],
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if drag.preview_id != cx.entity_id() {
            return;
        }
        let Some(origin) = &self.editor_drag else {
            return;
        };
        if origin.control.key() != drag.control_key {
            return;
        }
        cx.set_active_drag_cursor_style(origin.control.cursor(), window);
        let scalar = origin.control.scalar.clone();
        let value = origin.value_at(pointer);
        self.editor
            .update_if_changed(cx, |editor| Self::update_scalar(editor, &scalar, value));
    }

    fn update_scalar(editor: &mut TimelineEditor, scalar: &PreviewScalarValue, value: f32) -> bool {
        let address = &scalar.address;
        if editor.selected_item_ids().count() != 1 || !editor.is_item_selected(address.item_id) {
            return false;
        }
        let Some(value) = address.schema(editor).and_then(|schema| {
            schema
                .configuration_constraints(address.scalar_index)
                .clamp_value(&PropertyValue::F32(value))
        }) else {
            return false;
        };
        match scalar.stop {
            Some(index) => editor
                .property_animation_stop(address, index)
                .is_some_and(|stop| editor.set_property_animation_stop(&stop, value)),
            None => editor.update_property(EditScope::Item(address.item_id), address, value),
        }
    }
}
