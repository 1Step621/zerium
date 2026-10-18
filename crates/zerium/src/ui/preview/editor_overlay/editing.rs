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
        let targets = controls
            .iter()
            .map(|control| match control.scalar.stop {
                Some(index) => {
                    let address = &control.scalar.address;
                    let plan = editor
                        .property_animation_stops(address, time)?
                        .into_iter()
                        .find(|stop| stop.index() == index)
                        .or_else(|| editor.property_animation_stop(address, index))?;
                    Some(ValueEditTarget::Animation(plan))
                }
                None => Some(ValueEditTarget::Property(control.scalar.address.clone())),
            })
            .collect::<Option<Vec<_>>>();
        let Some(edit) = targets.and_then(|targets| editor.begin_value_gesture(targets)) else {
            return;
        };
        self.editor_drag = Some(PreviewEditorDrag {
            controls,
            pointer,
            composition_units_per_pixel,
            edit,
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
            if editor.active_selected_item_ids().count() != 1
                || origin
                    .controls
                    .iter()
                    .any(|control| !editor.is_item_active_selected(control.scalar.address.item_id))
            {
                return false;
            }
            let values = origin
                .controls
                .iter()
                .map(|control| {
                    let address = &control.scalar.address;
                    editor
                        .property_schema(address)?
                        .configuration_constraints(address.scalar_index)
                        .clamp_value(&PropertyValue::F32(control.value_at(delta)))
                })
                .collect::<Option<Vec<_>>>();
            values.is_some_and(|values| editor.update_value_edit(&mut origin.edit, &values))
        });
    }

    pub(in crate::ui::preview) fn end_editor_drag(&mut self, cx: &mut Context<Self>) {
        if self.editor_drag.take().is_some() {
            self.editor
                .update(cx, |editor, _| editor.finish_history_group());
            cx.notify();
        }
    }
}
