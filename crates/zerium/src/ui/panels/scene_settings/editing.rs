use super::*;
use crate::ui::components::input::set_input_text;

pub(super) struct NumericArgument {
    pub number: NumericInput,
    pub inputs: [Option<Entity<InputState>>; 3],
    pub values: NumericSettingDraft,
}

impl SceneSettings {
    pub(super) fn apply_text_input(
        &mut self,
        key: &ControlId,
        input: &Entity<InputState>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let ControlId::ArgumentSetting {
            scene_id,
            argument_id,
            setting,
        } = key
        {
            self.apply_scene_argument_settings(*scene_id, argument_id, *setting, window, cx);
            return;
        }
        let value = input.read(cx).value().to_string();
        if let ControlId::CalculationInputName {
            variable_id,
            input_name,
            ..
        } = key
        {
            let variable_id = variable_id.clone();
            let input_name = input_name.clone();
            self.store.inputs.set_draft(key, false);
            let accepted = self.edit_calculation_inputs(
                &variable_id,
                move |inputs| {
                    if let Some(input) = inputs.iter_mut().find(|input| input.name == input_name) {
                        input.name = value;
                    }
                },
                cx,
            );
            if !accepted {
                self.store.inputs.set_draft(key, true);
            }
            self.sync_inputs(window, cx);
            return;
        }
        self.editor.update(cx, |editor, cx| {
            let changed = match key {
                ControlId::Name(scene_id) => editor.rename_scene(*scene_id, value),
                ControlId::CalculationScript { variable_id, .. } => {
                    editor.update_scene_calculation_script(variable_id, value)
                }
                ControlId::ArgumentName { argument_id, .. } => {
                    editor.rename_scene_variable(argument_id, &value)
                }
                ControlId::ArgumentDefault { argument_id, .. } => {
                    editor.update_scene_argument_default(argument_id, PropertyValue::String(value))
                        == Ok(true)
                }
                _ => false,
            };
            if changed {
                cx.notify();
            }
        });
    }

    pub(super) fn read_numeric_argument(
        &self,
        scene_id: SceneId,
        argument_id: &str,
        cx: &App,
    ) -> Option<NumericArgument> {
        let editor = self.editor.read(cx);
        if editor.active_scene_id() != scene_id {
            return None;
        }
        let variable = editor.scene(scene_id)?.argument(argument_id)?;
        let number = NumericInput::for_schema(variable.argument_schema()?)?;
        let inputs = NumericSetting::ALL.map(|setting| {
            self.store.inputs.text(&ControlId::scene_argument_setting(
                scene_id,
                argument_id,
                setting,
            ))
        });
        inputs[NumericSetting::Default as usize].as_ref()?;
        let mut text = NumericSettingDraft::for_argument(variable)?
            .formatted(&number)
            .map(SharedString::from);
        for (text, input) in text.iter_mut().zip(&inputs) {
            if let Some(input) = input {
                *text = input.read(cx).value();
            }
        }
        let values = NumericSettingDraft::parse(&number, &text)?;
        Some(NumericArgument {
            number,
            inputs,
            values,
        })
    }

    pub(super) fn commit_numeric_argument(
        &mut self,
        argument_id: &str,
        argument: NumericArgument,
        changed: NumericSetting,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(values) = argument.values.normalize(&argument.number, changed) else {
            return;
        };
        let Some(settings) = values.to_domain(&argument.number) else {
            return;
        };
        let changed = self.editor.update(cx, |editor, cx| {
            let changed = editor.update_scene_argument_numeric_settings(argument_id, settings);
            if changed {
                cx.notify();
            }
            changed
        });
        if changed {
            for (input, text) in argument
                .inputs
                .iter()
                .zip(values.formatted(&argument.number))
            {
                if let Some(input) = input {
                    set_input_text(input, text, window, cx);
                }
            }
        }
    }

    pub(super) fn apply_scene_argument_settings(
        &mut self,
        scene_id: SceneId,
        argument_id: &str,
        changed: NumericSetting,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(argument) = self.read_numeric_argument(scene_id, argument_id, cx) {
            self.commit_numeric_argument(argument_id, argument, changed, window, cx);
        }
    }

    pub(super) fn step_scene_argument_setting(
        &mut self,
        scene_id: SceneId,
        argument_id: &str,
        setting: NumericSetting,
        event: &NumberInputEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(mut argument) = self.read_numeric_argument(scene_id, argument_id, cx) else {
            return;
        };
        let NumberInputEvent::Step { action, fine } = event;
        let step = argument.number.step(if *fine { 0.1 } else { 1. });
        let delta = if *action == StepAction::Increment {
            step
        } else {
            -step
        };
        let value = argument.values.value(setting) + delta;
        argument.values.set(setting, value);
        self.commit_numeric_argument(argument_id, argument, setting, window, cx);
    }
}

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
        event: &crate::ui::components::input::number::NumberDragStart,
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
        let Some(input) = &argument.inputs[setting as usize] else {
            return;
        };
        let input_id = input.entity_id();
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
        if event.event.pressed_button != Some(MouseButton::Left) {
            self.finish_number_drag(cx);
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
        if argument.inputs[origin.setting as usize]
            .as_ref()
            .is_none_or(|input| input.entity_id() != origin.input_id)
        {
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
