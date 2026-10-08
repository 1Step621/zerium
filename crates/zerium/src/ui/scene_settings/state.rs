use super::*;
use crate::ui::input::{InputControl, set_input_text};

pub(super) struct ColorState {
    pub picker: Entity<ColorPickerState>,
    pub _subscription: Subscription,
}

#[derive(Default)]
pub(super) struct SettingsStore {
    pub text_inputs: HashMap<ControlId, InputControl>,
    pub color_pickers: HashMap<ControlId, ColorState>,
    pub number_drag_origin: Option<Rc<NumberDragOrigin>>,
}

impl SettingsStore {
    pub(super) fn text(&self, id: &ControlId) -> Option<Entity<InputState>> {
        self.text_inputs.get(id).map(|state| state.input.clone())
    }
}

impl SceneSettings {
    fn color_to_hsla(color: [f32; 4]) -> gpui::Hsla {
        Rgba {
            r: color[0],
            g: color[1],
            b: color[2],
            a: color[3],
        }
        .into()
    }

    pub(super) fn sync_inputs(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some((scene_id, name, arguments)) = self
            .scene_id
            .and_then(|id| self.editor.read(cx).scene(id))
            .map(|scene| (scene.id, scene.name.clone(), scene.arguments.clone()))
        else {
            return;
        };
        let mut texts = vec![(ControlId::scene_name(scene_id), name)];
        let mut colors = Vec::new();
        self.expanded_scene_arguments
            .retain(|id| arguments.iter().any(|argument| argument.schema.id() == id));
        for argument in arguments {
            let argument_id = argument.schema.id();
            let label = if argument.schema.label().is_empty() {
                argument_id
            } else {
                argument.schema.label()
            };
            texts.push((
                ControlId::scene_argument_name(scene_id, argument_id),
                label.to_owned(),
            ));
            if let Some(number) = NumericInput::for_schema(&argument.schema) {
                if let Some(values) = NumericSettingDraft::for_schema(&argument.schema) {
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
                match argument.schema.default_value() {
                    PropertyValue::String(value) => texts.push((
                        ControlId::scene_argument_default(scene_id, argument_id),
                        value.clone(),
                    )),
                    PropertyValue::Color(value) => colors.push((argument_id.to_owned(), *value)),
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
        self.store.text_inputs.retain(|id, _| controls.contains(id));
        self.store
            .color_pickers
            .retain(|id, _| controls.contains(id));
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
        if let Some(input) = self.store.text(&key) {
            set_input_text(&input, value, window, cx);
            return;
        }
        let placeholder = match &key {
            ControlId::ArgumentSetting { setting, .. } => setting.placeholder(),
            _ => String::new(),
        };
        let input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(placeholder)
                .default_value(SharedString::from(value))
        });
        let edited_key = key.clone();
        let mut subscriptions =
            vec![
                cx.subscribe_in(&input, window, move |this, input, event, window, cx| {
                    if matches!(event, InputEvent::Change) {
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
            let (scene_id, argument_id, setting) = (*scene_id, argument_id.clone(), *setting);
            subscriptions.push(cx.subscribe_in(
                &input,
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
        self.store
            .text_inputs
            .insert(key, InputControl::new(input, subscriptions));
    }

    fn ensure_color(
        &mut self,
        scene_id: SceneId,
        argument_id: &str,
        value: [f32; 4],
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let color = Self::color_to_hsla(value);
        let color_key = ControlId::scene_argument_color(scene_id, argument_id);
        if let Some(picker) = self
            .store
            .color_pickers
            .get(&color_key)
            .map(|state| &state.picker)
        {
            if picker.read(cx).value() != Some(color) {
                picker.update(cx, |picker, cx| picker.set_value(color, window, cx));
            }
        } else {
            let picker = cx.new(|cx| ColorPickerState::new(window, cx).default_value(color));
            let edited_id = argument_id.to_owned();
            let subscription = cx.subscribe_in(&picker, window, move |this, _, event, _, cx| {
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
            });
            self.store.color_pickers.insert(
                color_key,
                ColorState {
                    picker,
                    _subscription: subscription,
                },
            );
        }
    }
}
