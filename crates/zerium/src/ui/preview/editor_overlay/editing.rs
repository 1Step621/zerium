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
        let editor = self.editor.read(cx);
        let time = TimelineTime::from_frame(editor.playhead());
        let controls = controls
            .into_iter()
            .map(|control| {
                let edit = match control.scalar.stop {
                    Some(index) => {
                        let address = &control.scalar.address;
                        Some(
                            editor
                                .property_animation_stops(address, time)?
                                .into_iter()
                                .find(|stop| stop.index() == index)
                                .or_else(|| editor.property_animation_stop(address, index))?,
                        )
                    }
                    None => None,
                };
                Some((control, edit))
            })
            .collect::<Option<Vec<_>>>();
        let Some(controls) = controls else {
            return;
        };
        let gesture = editor.begin_edit_gesture();
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
        let delta = pointer - origin.pointer;
        let delta = [f32::from(delta.x), f32::from(delta.y)]
            .map(|value| value * origin.composition_units_per_pixel);
        self.editor.update_if_changed(cx, |editor| {
            editor.edit_gesture(&mut origin.gesture, |editor| {
                for (control, edit) in &origin.controls {
                    Self::update_scalar(
                        editor,
                        &control.scalar,
                        edit.as_ref(),
                        control.value_at(delta),
                    );
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

    fn update_scalar(
        editor: &mut TimelineEditor,
        scalar: &PreviewScalarValue,
        edit: Option<&AnimationStopEdit>,
        value: f32,
    ) -> bool {
        let address = &scalar.address;
        if editor.active_selected_item_ids().count() != 1
            || !editor.is_item_active_selected(address.item_id)
        {
            return false;
        }
        let Some(value) = address.schema(editor).and_then(|schema| {
            schema
                .configuration_constraints(address.scalar_index)
                .clamp_value(&PropertyValue::F32(value))
        }) else {
            return false;
        };
        match edit {
            Some(stop) => editor.set_property_animation_stop(stop, value),
            None => editor.update_property(address, value),
        }
    }
}
