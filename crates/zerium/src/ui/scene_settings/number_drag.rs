use super::*;

#[derive(Clone)]
pub(super) struct NumberDragOrigin {
    scene_id: SceneId,
    argument_id: String,
    setting: NumericSetting,
    input_id: gpui::EntityId,
    adjustment: NumericDrag,
}

impl SceneSettings {
    pub(super) fn prepare_scene_argument_value_drag(
        &mut self,
        scene_id: SceneId,
        argument_id: &str,
        setting: NumericSetting,
        event: &MouseDownEvent,
        cx: &mut Context<Self>,
    ) {
        let Some(argument) = self.read_numeric_argument(scene_id, argument_id, cx) else {
            return;
        };
        let (min, max) = argument.values.bounds(&argument.number);
        if min > max {
            return;
        }
        let step = argument.number.step(1.);
        let input_id = argument.inputs[setting as usize].entity_id();
        self.editor
            .update(cx, |editor, _| editor.finish_history_group());
        self.store.number_drag_origin = Some(Rc::new(NumberDragOrigin {
            scene_id,
            argument_id: argument_id.to_owned(),
            setting,
            input_id,
            adjustment: NumericDrag {
                start_x: f32::from(event.position.x),
                start_value: argument.values.value(setting),
                step,
                sensitivity: ((max - min) / 200.).clamp(step * 0.1, step * 2.),
            },
        }));
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
            .clone()
            .filter(|origin| origin.input_id == drag.input_id)
        else {
            return;
        };
        let Some(mut argument) =
            self.read_numeric_argument(origin.scene_id, &origin.argument_id, cx)
        else {
            return;
        };
        if argument.inputs[origin.setting as usize].entity_id() != origin.input_id {
            return;
        }
        self.focus_handle.focus(window, cx);
        cx.set_active_drag_cursor_style(CursorStyle::ResizeLeftRight, window);
        let value = origin.adjustment.value_at(
            f32::from(event.event.position.x),
            event.event.modifiers.shift,
        );
        argument.values.set(origin.setting, value);
        self.commit_numeric_argument(&origin.argument_id, argument, origin.setting, window, cx);
    }

    pub(super) fn finish_number_drag(&mut self, cx: &mut Context<Self>) {
        self.store.number_drag_origin = None;
        self.editor
            .update(cx, |editor, _| editor.finish_history_group());
    }
}
