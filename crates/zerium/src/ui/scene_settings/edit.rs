use super::*;
use crate::ui::input::set_input_text;

pub(super) struct NumericArgument {
    pub number: NumericInput,
    pub inputs: [Entity<InputState>; 3],
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
        self.editor.update(cx, |editor, cx| {
            let changed = match key {
                ControlId::Name(scene_id) => editor.rename_scene(*scene_id, value),
                ControlId::ArgumentName { argument_id, .. } => {
                    editor.rename_scene_argument(argument_id, &value)
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
        if editor.active_scene_id() != Some(scene_id) {
            return None;
        }
        let schema = &editor
            .scene(scene_id)?
            .arguments
            .iter()
            .find(|argument| argument.schema.id() == argument_id)?
            .schema;
        let number = NumericInput::for_schema(schema)?;
        let [Some(default), Some(min), Some(max)] = NumericSetting::ALL.map(|setting| {
            self.store.text(&ControlId::scene_argument_setting(
                scene_id,
                argument_id,
                setting,
            ))
        }) else {
            return None;
        };
        let inputs = [default, min, max];
        let values = NumericSettingDraft::parse(
            &number,
            &inputs.each_ref().map(|input| input.read(cx).value()),
        )?;
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
                set_input_text(input, text, window, cx);
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
