use super::*;
use crate::ui::components::input::{InputStore, TextOptions, TextSync, color_to_hsla};

#[derive(Default)]
pub(super) struct SettingsStore {
    pub inputs: InputStore<ControlId>,
    pub number_drag_origin: Option<Rc<NumberDragOrigin>>,
}

impl SceneSettings {
    pub(super) fn sync_inputs(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let scene = self.editor.read(cx).active_scene();
        let (scene_id, name, arguments) =
            (scene.id, scene.name.clone(), scene.variables().to_vec());
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
                                    this.store.inputs.set_draft(&edited_key, true);
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

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) enum NumericSetting {
    Default,
    Min,
    Max,
}

impl NumericSetting {
    pub(super) const ALL: [Self; 3] = [Self::Default, Self::Min, Self::Max];

    pub(super) fn placeholder(self) -> String {
        match self {
            Self::Default => String::new(),
            Self::Min => t!("args.no_minimum").to_string(),
            Self::Max => t!("args.no_maximum").to_string(),
        }
    }
}

#[derive(Clone, Copy)]
pub(super) struct NumericSettingDraft {
    default: f64,
    min: Option<f64>,
    max: Option<f64>,
}

impl NumericSettingDraft {
    pub(super) fn for_argument(argument: &zerium_core::timeline::SceneVariable) -> Option<Self> {
        let schema = argument.argument_schema()?;
        let constraints = argument.argument_limits()?;
        let ty = schema.scalar_type(None, None)?;
        let (min, max) = constraints.numeric_bounds(ty)?;
        Some(Self {
            default: schema.default_value().numeric_scalar()?,
            min: constraints.min.map(|_| min),
            max: constraints.max.map(|_| max),
        })
    }

    pub(super) fn parse(number: &NumericInput, text: &[SharedString; 3]) -> Option<Self> {
        let bound = |text: &str| {
            if text.trim().is_empty() {
                Some(None)
            } else {
                number.parse_number(text).map(Some)
            }
        };
        Some(Self {
            default: number.parse_number(&text[0])?,
            min: bound(&text[1])?,
            max: bound(&text[2])?,
        })
    }

    pub(super) fn value(self, setting: NumericSetting) -> f64 {
        match setting {
            NumericSetting::Default => self.default,
            NumericSetting::Min => self.min.unwrap_or(self.default),
            NumericSetting::Max => self.max.unwrap_or(self.default),
        }
    }

    pub(super) fn set(&mut self, setting: NumericSetting, value: f64) {
        match setting {
            NumericSetting::Default => self.default = value,
            NumericSetting::Min => self.min = Some(value),
            NumericSetting::Max => self.max = Some(value),
        }
    }

    pub(super) fn bounds(self, number: &NumericInput) -> (f64, f64) {
        let (lower, upper) = number.bounds();
        (self.min.unwrap_or(lower), self.max.unwrap_or(upper))
    }

    pub(super) fn normalize(
        mut self,
        number: &NumericInput,
        changed: NumericSetting,
    ) -> Option<Self> {
        // Convert adjustments at the input's precision before comparing dependent fields.
        let (lower, upper) = number.bounds();
        let constrain = |value: f64| {
            value.is_finite().then_some(())?;
            number
                .value_from_number(value.clamp(lower, upper))?
                .numeric_scalar()
        };
        self.default = constrain(self.default)?;
        let optional = |value| match value {
            Some(value) => constrain(value).map(Some),
            None => Some(None),
        };
        self.min = optional(self.min)?;
        self.max = optional(self.max)?;

        let (min, max) = self.bounds(number);
        if min > max {
            match changed {
                NumericSetting::Min => self.min = Some(max),
                NumericSetting::Max => self.max = Some(min),
                NumericSetting::Default => return None,
            }
        }
        Some(self)
    }

    pub(super) fn to_domain(
        self,
        number: &NumericInput,
    ) -> Option<zerium_core::property::NumericSettings> {
        let default = number.value_from_number(self.default)?;
        let min = match self.min {
            Some(value) => Some(number.value_from_number(value)?),
            None => None,
        };
        let max = match self.max {
            Some(value) => Some(number.value_from_number(value)?),
            None => None,
        };
        zerium_core::property::NumericSettings::from_values(default, min, max)
    }

    pub(super) fn formatted(self, number: &NumericInput) -> [String; 3] {
        [
            number.format(self.default),
            self.min
                .map(|value| number.format(value))
                .unwrap_or_default(),
            self.max
                .map(|value| number.format(value))
                .unwrap_or_default(),
        ]
    }
}
