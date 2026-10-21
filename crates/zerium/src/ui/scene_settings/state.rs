use super::*;
use crate::ui::input::color_to_hsla;
use crate::ui::input::{InputStore, TextOptions, TextSync};

#[derive(Default)]
pub(super) struct SettingsStore {
    pub inputs: InputStore<ControlId>,
    pub number_drag_origin: Option<Rc<NumberDragOrigin>>,
}

impl SceneSettings {
    pub(super) fn sync_inputs(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some((scene_id, name, arguments)) = self
            .scene_id
            .and_then(|id| self.editor.read(cx).scene(id))
            .map(|scene| (scene.id, scene.name.clone(), scene.variables().to_vec()))
        else {
            return;
        };
        let mut texts = vec![(ControlId::scene_name(scene_id), name)];
        let mut colors = Vec::new();
        for argument in arguments {
            let argument_id = argument.id();
            let label = argument.label();
            texts.push((
                ControlId::scene_argument_name(scene_id, argument_id),
                label.to_owned(),
            ));
            if let Some(calculation) = argument.calculation() {
                texts.push((
                    ControlId::CalculationScript {
                        scene_id,
                        variable_id: argument_id.to_owned(),
                    },
                    calculation.script.source().to_owned(),
                ));
                for input in &calculation.inputs {
                    texts.push((
                        ControlId::CalculationInputName {
                            scene_id,
                            variable_id: argument_id.to_owned(),
                            input_name: input.name.clone(),
                        },
                        input.name.clone(),
                    ));
                }
                continue;
            }
            let schema = argument.argument_schema().expect("argument row");
            if let Some(number) = NumericInput::for_schema(schema) {
                if let Some(values) = NumericSettingDraft::for_argument(&argument) {
                    for (setting, text) in NumericSetting::ALL
                        .into_iter()
                        .zip(values.formatted(&number))
                    {
                        texts.push((
                            ControlId::scene_argument_setting(scene_id, argument_id, setting),
                            text,
                        ));
                    }
                }
            } else {
                match schema.default_value() {
                    PropertyValue::String(value) => texts.push((
                        ControlId::scene_argument_default(scene_id, argument_id),
                        value.clone(),
                    )),
                    PropertyValue::Color(value) => colors.push((argument_id.to_owned(), value)),
                    _ => {}
                }
            }
        }
        let controls: HashSet<_> = texts
            .iter()
            .map(|(id, _)| id.clone())
            .chain(
                colors
                    .iter()
                    .map(|(id, _)| ControlId::scene_argument_color(scene_id, id)),
            )
            .collect();
        self.store.inputs.retain(&controls);
        for (id, text) in texts {
            self.ensure_text(id, text, window, cx);
        }
        for (id, color) in colors {
            self.ensure_color(scene_id, &id, color, window, cx);
        }
    }

    fn ensure_text(
        &mut self,
        key: ControlId,
        value: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let placeholder = match &key {
            ControlId::ArgumentSetting { setting, .. } => setting.placeholder(),
            _ => String::new(),
        };
        let options = TextOptions {
            multiline: matches!(&key, ControlId::CalculationScript { .. }).then_some((3, 12)),
            placeholder,
            sync: TextSync::PreserveDraft,
        };
        self.store.inputs.ensure_text(
            key.clone(),
            value,
            options,
            |input, window, cx| {
                let edited_key = key.clone();
                let mut subscriptions =
                    vec![
                        cx.subscribe_in(input, window, move |this, input, event, window, cx| {
                            let commit = if let ControlId::CalculationInputName { .. } = &edited_key
                            {
                                if let InputEvent::Change = event {
                                    this.store.inputs.drafts.insert(edited_key.clone());
                                }
                                matches!(event, InputEvent::Blur | InputEvent::PressEnter { .. })
                            } else {
                                matches!(event, InputEvent::Change)
                            };
                            if commit {
                                this.apply_text_input(&edited_key, input, window, cx);
                            }
                        }),
                    ];
                if let ControlId::ArgumentSetting {
                    scene_id,
                    argument_id,
                    setting,
                } = &key
                {
                    let (scene_id, argument_id, setting) =
                        (*scene_id, argument_id.clone(), *setting);
                    subscriptions.push(cx.subscribe_in(
                        input,
                        window,
                        move |this, _, event: &NumberInputEvent, window, cx| {
                            this.step_scene_argument_setting(
                                scene_id,
                                &argument_id,
                                setting,
                                event,
                                window,
                                cx,
                            );
                        },
                    ));
                }
                subscriptions
            },
            window,
            cx,
        );
    }

    fn ensure_color(
        &mut self,
        scene_id: SceneId,
        argument_id: &str,
        value: [f32; 4],
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let color = color_to_hsla(value);
        let color_key = ControlId::scene_argument_color(scene_id, argument_id);
        let edited_id = argument_id.to_owned();
        self.store.inputs.ensure_color(
            color_key,
            color,
            |picker, window, cx| {
                cx.subscribe_in(picker, window, move |this, _, event, _, cx| {
                    let ColorPickerEvent::Change(Some(color)) = event else {
                        return;
                    };
                    let color = Rgba::from(*color);
                    let value = PropertyValue::Color([color.r, color.g, color.b, color.a]);
                    this.editor.update(cx, |editor, cx| {
                        if editor.update_scene_argument_default(&edited_id, value) == Ok(true) {
                            cx.notify();
                        }
                    });
                })
            },
            window,
            cx,
        );
    }
}
