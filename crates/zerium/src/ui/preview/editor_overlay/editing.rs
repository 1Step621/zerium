use super::*;
use crate::ui::TimelineEditorEntityExt as _;

impl Preview {
    pub(super) fn begin_editor_drag(
        &mut self,
        controls: Vec<PreviewScalarControl>,
        pointer: Point<Pixels>,
        composition_units_per_pixel: f32,
        cx: &mut Context<Self>,
    ) {
        if controls.is_empty() || composition_units_per_pixel <= 0. {
            return;
        }
        let gesture = self
            .editor
            .update(cx, |editor, _| editor.begin_edit_gesture());
        self.editor_drag = Some(PreviewEditorDrag {
            controls,
            pointer,
            composition_units_per_pixel,
            gesture,
        });
        cx.notify();
    }

    pub(super) fn move_editor_controls_from_pointer(
        &mut self,
        pointer: Point<Pixels>,
        cx: &mut Context<Self>,
    ) {
        let Some(origin) = &mut self.editor_drag else {
            return;
        };
        let updates = origin
            .controls
            .iter()
            .map(|control| (control.scalar.clone(), origin.value_at(control, pointer)))
            .collect::<Vec<_>>();
        self.editor.update_if_changed(cx, |editor| {
            editor.edit_gesture(&mut origin.gesture, |editor| {
                for (scalar, value) in updates {
                    Self::update_scalar(editor, &scalar, value);
                }
            })
        });
    }

    pub(in crate::ui::preview) fn end_editor_drag(&mut self, cx: &mut Context<Self>) {
        if self.editor_drag.take().is_some() {
            self.editor
                .update(cx, |editor, _| editor.finish_history_group());
            cx.notify();
        }
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
            None => editor.update_property(address, value),
        }
    }
}
