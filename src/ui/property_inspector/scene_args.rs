use super::rows::RenderCtx;
use super::*;

impl PropertyInspector {
    pub(super) fn toggle_scene_argument_expanded(
        &mut self,
        scene_id: SceneId,
        argument_id: &str,
        cx: &mut Context<Self>,
    ) {
        let key = (scene_id, argument_id.to_owned());
        if !self.expanded_scene_arguments.insert(key.clone()) {
            self.expanded_scene_arguments.remove(&key);
        }
        cx.notify();
    }

    fn scene_name_input(&self, scene_id: SceneId) -> Option<Entity<InputState>> {
        self.store
            .states
            .get(&ControlId::scene_name(scene_id))
            .and_then(state::ControlState::text)
            .map(|state| state.input.clone())
    }

    pub(super) fn ensure_active_scene_argument_names(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(scene_id) = self.editor.read(cx).active_scene_id() else {
            return;
        };
        let Some((scene_name, arguments)) = self
            .editor
            .read(cx)
            .scene(scene_id)
            .map(|scene| (scene.name.clone(), scene.arguments.clone()))
        else {
            return;
        };
        let key = ControlId::scene_name(scene_id);
        if self
            .store
            .states
            .get(&key)
            .and_then(state::ControlState::text)
            .is_none()
        {
            let input = cx.new(|cx| {
                InputState::new(window, cx).default_value(SharedString::from(scene_name.clone()))
            });
            let subscription = cx.subscribe_in(&input, window, move |this, input, event, _, cx| {
                if !matches!(event, InputEvent::Change) {
                    return;
                }
                let name = input.read(cx).value().to_string();
                this.editor.update(cx, |editor, cx| {
                    if editor.rename_scene(scene_id, name) {
                        cx.notify();
                    }
                });
            });
            self.store.states.insert(
                key,
                state::ControlState::Text(state::TextState {
                    input,
                    _subscriptions: vec![subscription],
                }),
            );
        } else {
            let input = self.scene_name_input(scene_id).expect("name input ensured");
            Self::set_input_value(&input, scene_name, window, cx);
        }
        let displayed_expressions = arguments
            .iter()
            .filter_map(|argument| {
                Some((
                    argument.schema.id().to_owned(),
                    display_scene_expression(&arguments, argument.expression()?),
                ))
            })
            .collect::<HashMap<_, _>>();
        for argument in arguments {
            let argument_id = argument.schema.id().to_owned();
            let label = if argument.schema.label().is_empty() {
                argument_id.clone()
            } else {
                argument.schema.label().to_owned()
            };
            let name_key = ControlId::scene_argument_name(scene_id, &argument_id);
            if self
                .store
                .states
                .get(&name_key)
                .and_then(state::ControlState::text)
                .is_none()
            {
                let input = cx
                    .new(|cx| InputState::new(window, cx).default_value(SharedString::from(label)));
                let renamed_id = argument_id.clone();
                let subscription =
                    cx.subscribe_in(&input, window, move |this, input, event, _, cx| {
                        if !matches!(event, InputEvent::Change) {
                            return;
                        }
                        let label = input.read(cx).value().to_string();
                        this.editor.update(cx, |editor, cx| {
                            if editor.rename_scene_argument(&renamed_id, &label) {
                                cx.notify();
                            }
                        });
                    });
                self.store.states.insert(
                    name_key,
                    state::ControlState::Text(state::TextState {
                        input,
                        _subscriptions: vec![subscription],
                    }),
                );
            } else {
                let input = self
                    .store
                    .states
                    .get(&name_key)
                    .and_then(state::ControlState::text)
                    .map(|state| state.input.clone())
                    .expect("argument name input ensured");
                Self::set_input_value(&input, label, window, cx);
            }

            if let Some(expression) = displayed_expressions.get(&argument_id).cloned() {
                let expression_key = ControlId::scene_argument_expression(scene_id, &argument_id);
                if self
                    .store
                    .states
                    .get(&expression_key)
                    .and_then(state::ControlState::text)
                    .is_none()
                {
                    let input = cx.new(|cx| {
                        InputState::new(window, cx).default_value(SharedString::from(expression))
                    });
                    let edited_id = argument_id.clone();
                    let subscription =
                        cx.subscribe_in(&input, window, move |this, input, event, _, cx| {
                            if !matches!(event, InputEvent::Change) {
                                return;
                            }
                            let expression = input.read(cx).value().to_string();
                            this.editor.update(cx, |editor, cx| {
                                if editor.update_scene_argument_expression(&edited_id, &expression)
                                {
                                    cx.notify();
                                }
                            });
                        });
                    self.store.states.insert(
                        expression_key,
                        state::ControlState::Text(state::TextState {
                            input,
                            _subscriptions: vec![subscription],
                        }),
                    );
                } else {
                    let input = self
                        .store
                        .states
                        .get(&expression_key)
                        .and_then(state::ControlState::text)
                        .map(|state| state.input.clone())
                        .expect("argument expression input ensured");
                    Self::set_input_value(&input, expression, window, cx);
                }
                continue;
            }

            if let Some(number) = NumericInput::for_schema(argument.schema.declared_parameter()) {
                let Some(values) = numeric_settings(&number, argument.schema.declared_parameter())
                else {
                    continue;
                };
                for (setting, value) in [
                    SceneArgumentSetting::Default,
                    SceneArgumentSetting::Min,
                    SceneArgumentSetting::Max,
                ]
                .into_iter()
                .zip(values)
                {
                    self.ensure_scene_argument_setting_input(
                        scene_id,
                        &argument_id,
                        setting,
                        value,
                        window,
                        cx,
                    );
                }
            } else if *argument.schema.ty()
                == ParameterType::Value(ParameterValueType::Scalar(ScalarParameterType::String))
            {
                let value = match argument.schema.default_value() {
                    ParameterValue::String(value) => value.clone(),
                    _ => String::new(),
                };
                let default_key = ControlId::scene_argument_default(scene_id, &argument_id);
                if self
                    .store
                    .states
                    .get(&default_key)
                    .and_then(state::ControlState::text)
                    .is_none()
                {
                    let input = cx.new(|cx| {
                        InputState::new(window, cx).default_value(SharedString::from(value))
                    });
                    let edited_id = argument_id.clone();
                    let subscription =
                        cx.subscribe_in(&input, window, move |this, input, event, _, cx| {
                            if !matches!(event, InputEvent::Change) {
                                return;
                            }
                            let value = input.read(cx).value().to_string();
                            this.editor.update(cx, |editor, cx| {
                                if editor.update_scene_argument_default(
                                    &edited_id,
                                    ParameterValue::String(value),
                                ) {
                                    cx.notify();
                                }
                            });
                        });
                    self.store.states.insert(
                        default_key,
                        state::ControlState::Text(state::TextState {
                            input,
                            _subscriptions: vec![subscription],
                        }),
                    );
                } else {
                    let input = self
                        .store
                        .states
                        .get(&default_key)
                        .and_then(state::ControlState::text)
                        .map(|state| state.input.clone())
                        .expect("argument default input ensured");
                    Self::set_input_value(&input, value, window, cx);
                }
            } else if matches!(
                argument.schema.ty().scalar_type(),
                Some(ScalarParameterType::Color)
            ) {
                let value = match argument.schema.default_value() {
                    ParameterValue::Color(value) => *value,
                    _ => [0., 0., 0., 1.],
                };
                let color = Self::color_to_hsla(value);
                let color_key = ControlId::scene_argument_color(scene_id, &argument_id);
                if let Some(picker) = self
                    .store
                    .states
                    .get(&color_key)
                    .and_then(state::ControlState::color)
                    .map(|state| &state.picker)
                {
                    if picker.read(cx).value() != Some(color) {
                        picker.update(cx, |picker, cx| picker.set_value(color, window, cx));
                    }
                } else {
                    let picker =
                        cx.new(|cx| ColorPickerState::new(window, cx).default_value(color));
                    let edited_id = argument_id.clone();
                    let subscription =
                        cx.subscribe_in(&picker, window, move |this, _, event, _, cx| {
                            let ColorPickerEvent::Change(Some(color)) = event else {
                                return;
                            };
                            let color = Rgba::from(*color);
                            let value = ParameterValue::Color([color.r, color.g, color.b, color.a]);
                            this.editor.update(cx, |editor, cx| {
                                if editor.update_scene_argument_default(&edited_id, value) {
                                    cx.notify();
                                }
                            });
                        });
                    self.store.states.insert(
                        color_key,
                        state::ControlState::Color {
                            picker: state::ColorState {
                                picker,
                                _subscriptions: vec![subscription],
                            },
                            animation: None,
                        },
                    );
                }
            }
        }
    }

    pub(super) fn ensure_scene_argument_setting_input(
        &mut self,
        scene_id: SceneId,
        argument_id: &str,
        setting: SceneArgumentSetting,
        value: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let key = ControlId::scene_argument_setting(scene_id, argument_id, setting);
        if self
            .store
            .states
            .get(&key)
            .and_then(state::ControlState::text)
            .is_some()
        {
            let input = self
                .store
                .states
                .get(&key)
                .and_then(state::ControlState::text)
                .map(|state| state.input.clone())
                .expect("setting input ensured");
            Self::set_input_value(&input, value, window, cx);
            return;
        }
        let input =
            cx.new(|cx| InputState::new(window, cx).default_value(SharedString::from(value)));
        let step_argument_id = argument_id.to_owned();
        let step_sub = cx.subscribe_in(
            &input,
            window,
            move |this, input, event: &NumberInputEvent, window, cx| {
                this.step_scene_argument_setting(
                    scene_id,
                    &step_argument_id,
                    setting,
                    input,
                    event,
                    window,
                    cx,
                );
            },
        );
        let argument_id = argument_id.to_owned();
        let change_sub = cx.subscribe_in(&input, window, move |this, _, event, _, cx| {
            if matches!(event, InputEvent::Change) {
                this.apply_scene_argument_settings(scene_id, &argument_id, cx);
            }
        });
        self.store.states.insert(
            key,
            state::ControlState::Text(state::TextState {
                input,
                _subscriptions: vec![step_sub, change_sub],
            }),
        );
    }

    #[allow(clippy::too_many_arguments)]
    fn step_scene_argument_setting(
        &mut self,
        scene_id: SceneId,
        argument_id: &str,
        setting: SceneArgumentSetting,
        input: &Entity<InputState>,
        event: &NumberInputEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(number) = self
            .editor
            .read(cx)
            .scene(scene_id)
            .and_then(|scene| {
                scene
                    .arguments
                    .iter()
                    .find(|argument| argument.schema.id() == argument_id)
            })
            .and_then(|argument| NumericInput::for_schema(argument.schema.declared_parameter()))
        else {
            return;
        };
        let setting_text = |setting| {
            self.store
                .states
                .get(&ControlId::scene_argument_setting(
                    scene_id,
                    argument_id,
                    setting,
                ))
                .and_then(state::ControlState::text)
                .map(|state| state.input.read(cx).value())
        };
        let parse = |setting| {
            setting_text(setting).and_then(|text| setting_display_value(&number, setting, &text))
        };
        let (Some(current), Some(min), Some(max)) = (
            parse(setting),
            parse(SceneArgumentSetting::Min),
            parse(SceneArgumentSetting::Max),
        ) else {
            return;
        };
        let NumberInputEvent::Step { action, fine } = event;
        let delta = number.step(if *fine { 0.1 } else { 1. })
            * if *action == StepAction::Increment {
                1.
            } else {
                -1.
            };
        let Some(value) = clamp_setting(&number, setting, current + delta, min, max) else {
            return;
        };
        Self::set_input_value(input, number.format(value), window, cx);
        self.apply_scene_argument_settings(scene_id, argument_id, cx);
    }

    pub(super) fn apply_scene_argument_settings(
        &mut self,
        scene_id: SceneId,
        argument_id: &str,
        cx: &mut Context<Self>,
    ) {
        if self.editor.read(cx).active_scene_id() != Some(scene_id) {
            return;
        }
        let Some(number) = self
            .editor
            .read(cx)
            .scene(scene_id)
            .and_then(|scene| {
                scene
                    .arguments
                    .iter()
                    .find(|argument| argument.schema.id() == argument_id)
            })
            .and_then(|argument| NumericInput::for_schema(argument.schema.declared_parameter()))
        else {
            return;
        };
        let text = |setting| {
            self.store
                .states
                .get(&ControlId::scene_argument_setting(
                    scene_id,
                    argument_id,
                    setting,
                ))
                .and_then(state::ControlState::text)
                .map(|state| state.input.read(cx).value().to_string())
        };
        let Some(settings) = (|| {
            let default = number.parse(&text(SceneArgumentSetting::Default)?)?;
            let min = number.parse_optional(&text(SceneArgumentSetting::Min)?)?;
            let max = number.parse_optional(&text(SceneArgumentSetting::Max)?)?;
            crate::domain::parameter::NumericSettings::from_values(default, min, max)
        })() else {
            return;
        };
        self.editor.update(cx, |editor, cx| {
            if editor.update_scene_argument_numeric_settings(argument_id, settings) {
                cx.notify();
            }
        });
    }

    pub(super) fn prepare_scene_argument_value_drag(
        &mut self,
        drag: &SceneArgumentValueDrag,
        event: &MouseDownEvent,
        cx: &mut Context<Self>,
    ) {
        let Some(number) = self
            .editor
            .read(cx)
            .scene(drag.scene_id)
            .and_then(|scene| {
                scene
                    .arguments
                    .iter()
                    .find(|argument| argument.schema.id() == drag.argument_id)
            })
            .and_then(|argument| NumericInput::for_schema(argument.schema.declared_parameter()))
        else {
            return;
        };
        let input = self.store.states.get(&ControlId::scene_argument_setting(
            drag.scene_id,
            &drag.argument_id,
            drag.setting,
        ));
        let Some(start_value) = input.and_then(|field| {
            state::ControlState::text(field).and_then(|state| {
                setting_display_value(&number, drag.setting, &state.input.read(cx).value())
            })
        }) else {
            return;
        };
        let parse = |setting| {
            self.store
                .states
                .get(&ControlId::scene_argument_setting(
                    drag.scene_id,
                    &drag.argument_id,
                    setting,
                ))
                .and_then(state::ControlState::text)
                .map(|state| state.input.read(cx).value())
                .and_then(|text| setting_display_value(&number, setting, &text))
        };
        let (Some(min), Some(max)) = (
            parse(SceneArgumentSetting::Min),
            parse(SceneArgumentSetting::Max),
        ) else {
            return;
        };

        if !min.is_finite() || !max.is_finite() || min > max || !start_value.is_finite() {
            return;
        }
        let step = number.step(1.);
        self.editor
            .update(cx, |editor, _| editor.finish_history_group());
        self.store.scene_argument_value_drag_origin = Some(SceneArgumentValueDragOrigin {
            scene_id: drag.scene_id,
            argument_id: drag.argument_id.clone(),
            setting: drag.setting,
            start_x: f32::from(event.position.x),
            start_value,
            sensitivity: ((max - min) / Self::DRAG_RANGE_PIXELS).clamp(step * 0.1, step * 2.),
            number,
        });
    }

    pub(super) fn handle_scene_argument_value_drag(
        &mut self,
        drag: &SceneArgumentValueDrag,
        pointer_x: f32,
        fine_adjustment: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if drag.inspector_id != cx.entity_id() {
            return;
        }
        let Some(origin) = self
            .store
            .scene_argument_value_drag_origin
            .as_ref()
            .filter(|origin| {
                origin.scene_id == drag.scene_id
                    && origin.argument_id == drag.argument_id
                    && origin.setting == drag.setting
            })
        else {
            return;
        };
        let step = origin.number.step(if fine_adjustment { 0.1 } else { 1. });
        let value = ((origin.start_value
            + f64::from(pointer_x - origin.start_x)
                * origin.sensitivity
                * if fine_adjustment { 0.1 } else { 1. })
            / step)
            .round()
            * step;
        let parse = |setting| {
            self.store
                .states
                .get(&ControlId::scene_argument_setting(
                    drag.scene_id,
                    &drag.argument_id,
                    setting,
                ))
                .and_then(state::ControlState::text)
                .map(|state| state.input.read(cx).value())
                .and_then(|text| setting_display_value(&origin.number, setting, &text))
        };
        let (Some(min), Some(max)) = (
            parse(SceneArgumentSetting::Min),
            parse(SceneArgumentSetting::Max),
        ) else {
            return;
        };
        if !min.is_finite() || !max.is_finite() || min > max {
            return;
        }
        let Some(value) = clamp_setting(&origin.number, drag.setting, value, min, max) else {
            return;
        };
        let Some(input) = self
            .store
            .states
            .get(&ControlId::scene_argument_setting(
                drag.scene_id,
                &drag.argument_id,
                drag.setting,
            ))
            .and_then(state::ControlState::text)
        else {
            return;
        };
        Self::set_input_value(&input.input, origin.number.format(value), window, cx);
        self.apply_scene_argument_settings(drag.scene_id, &drag.argument_id, cx);
    }

    pub(super) fn scene_settings_element(
        &self,
        arguments: &[SceneArgumentOption],
        render: &RenderCtx<'_>,
    ) -> gpui::AnyElement {
        let active_scene_name_input = render.active_scene_name_input.clone();
        let rows = arguments
            .iter()
            .cloned()
            .enumerate()
            .map(|(index, argument)| {
                self.scene_argument_element(index, arguments.len(), argument, render)
            })
            .collect::<Vec<_>>();

        let mut section = div()
            .w_full()
            .min_w_0()
            .flex()
            .flex_col()
            .gap_2()
            .p_2()
            .rounded_sm()
            .border_1()
            .border_color(render.colors.border)
            .bg(render.colors.muted.opacity(0.18))
            .child(
                div()
                    .text_sm()
                    .text_color(render.colors.muted_foreground)
                    .child("シーン設定"),
            );
        if let Some(input) = active_scene_name_input {
            section = section.child(Self::scene_text_input_row("シーン名", input));
        }
        section
            .child(Self::scene_arguments_header(render.editor))
            .children(rows)
            .into_any_element()
    }

    fn scene_arguments_header(editor: &Entity<TimelineEditor>) -> Div {
        let create_editor = editor.clone();
        div()
            .w_full()
            .flex()
            .items_center()
            .justify_between()
            .child(div().text_sm().child("シーン引数"))
            .child(
                Button::new("create-scene-argument")
                    .small()
                    .compact()
                    .label("引数を作成")
                    .dropdown_caret(true)
                    .popup_menu(move |menu, _, _| {
                        [
                            ("小数", SceneArgumentPreset::Number),
                            ("整数", SceneArgumentPreset::SignedInteger),
                            ("非負の整数", SceneArgumentPreset::UnsignedInteger),
                            ("真偽値", SceneArgumentPreset::Boolean),
                            ("色", SceneArgumentPreset::Color),
                            ("文字列", SceneArgumentPreset::Text),
                        ]
                        .into_iter()
                        .fold(menu, |menu, (label, ty)| {
                            let editor = create_editor.clone();
                            menu.item(PopupMenuItem::new(label).on_click(move |_, _, cx| {
                                editor.update(cx, |editor, cx| {
                                    if editor.create_scene_argument(ty).is_some() {
                                        cx.notify();
                                    }
                                });
                            }))
                        })
                        .item(PopupMenuItem::new("数値(導出)").on_click({
                            let editor = create_editor.clone();
                            move |_, _, cx| {
                                editor.update(cx, |editor, cx| {
                                    if editor.create_derived_scene_argument().is_some() {
                                        cx.notify();
                                    }
                                });
                            }
                        }))
                    }),
            )
    }

    fn scene_argument_element(
        &self,
        index: usize,
        count: usize,
        argument: SceneArgumentOption,
        render: &RenderCtx<'_>,
    ) -> gpui::AnyElement {
        let expanded = self
            .expanded_scene_arguments
            .contains(&(argument.scene_id, argument.id.clone()));
        let name_input = self
            .store
            .states
            .get(&ControlId::scene_argument_name(
                argument.scene_id,
                &argument.id,
            ))
            .and_then(state::ControlState::text)
            .map(|state| state.input.clone());
        let details = expanded.then(|| self.scene_argument_details(&argument, render));

        div()
            .w_full()
            .min_w_0()
            .flex()
            .flex_col()
            .flex_none()
            .gap_2()
            .child(Self::scene_argument_header(
                index, count, &argument, name_input, expanded, render,
            ))
            .when_some(details, |this, details| this.child(details))
            .into_any_element()
    }

    fn scene_argument_header(
        index: usize,
        count: usize,
        argument: &SceneArgumentOption,
        name_input: Option<Entity<InputState>>,
        expanded: bool,
        render: &RenderCtx<'_>,
    ) -> Div {
        let move_up_editor = render.editor.clone();
        let move_up_id = argument.id.clone();
        let move_down_editor = render.editor.clone();
        let move_down_id = argument.id.clone();
        let expand_inspector = render.inspector.clone();
        let expand_id = argument.id.clone();
        let remove_inspector = render.inspector.clone();
        let remove_id = argument.id.clone();
        let scene_id = argument.scene_id;

        div()
            .w_full()
            .min_w_0()
            .flex()
            .flex_none()
            .items_center()
            .gap_2()
            .child(
                div()
                    .w_0()
                    .min_w_0()
                    .flex_1()
                    .when_some(name_input, |this, input| {
                        this.child(Input::new(&input).small().w_full())
                    }),
            )
            .child(
                div()
                    .flex_none()
                    .whitespace_nowrap()
                    .text_sm()
                    .text_color(render.colors.muted_foreground)
                    .child(format!("{}件接続", argument.binding_count)),
            )
            .child(
                Button::new(SharedString::from(format!(
                    "move-scene-argument-up-{}",
                    argument.id
                )))
                .small()
                .compact()
                .flex_none()
                .ghost()
                .icon(IconName::ChevronUp)
                .tooltip("上へ移動")
                .disabled(index == 0)
                .on_click(move |_, _, cx| {
                    move_up_editor.update(cx, |editor, cx| {
                        if editor.move_scene_argument(&move_up_id, -1) {
                            cx.notify();
                        }
                    });
                }),
            )
            .child(
                Button::new(SharedString::from(format!(
                    "move-scene-argument-down-{}",
                    argument.id
                )))
                .small()
                .compact()
                .flex_none()
                .ghost()
                .icon(IconName::ChevronDown)
                .tooltip("下へ移動")
                .disabled(index + 1 == count)
                .on_click(move |_, _, cx| {
                    move_down_editor.update(cx, |editor, cx| {
                        if editor.move_scene_argument(&move_down_id, 1) {
                            cx.notify();
                        }
                    });
                }),
            )
            .child(
                Button::new(SharedString::from(format!(
                    "toggle-scene-argument-settings-{}",
                    argument.id
                )))
                .small()
                .compact()
                .flex_none()
                .ghost()
                .icon(if expanded {
                    IconName::ChevronDown
                } else {
                    IconName::ChevronRight
                })
                .tooltip(if expanded {
                    "詳細設定を折り畳む"
                } else {
                    "詳細設定を展開"
                })
                .on_click(move |_, _, cx| {
                    expand_inspector.update(cx, |inspector, cx| {
                        inspector.toggle_scene_argument_expanded(scene_id, &expand_id, cx);
                    });
                }),
            )
            .child(
                Button::new(SharedString::from(format!(
                    "remove-scene-argument-{}",
                    argument.id
                )))
                .small()
                .compact()
                .flex_none()
                .ghost()
                .icon(IconName::Delete)
                .tooltip(if argument.referenced_by_derived {
                    "式から参照されているため削除できません"
                } else {
                    "引数を削除"
                })
                .disabled(argument.referenced_by_derived)
                .on_click(move |_, _, cx| {
                    remove_inspector.update(cx, |inspector, cx| {
                        let result = inspector.editor.update(cx, |editor, cx| {
                            let result = editor.remove_scene_argument(&remove_id);
                            if result.is_ok() {
                                cx.notify();
                            }
                            result
                        });
                        if result.is_err() {
                            inspector.notifications.update(cx, |notifications, cx| {
                                notifications.push("シーン引数を削除できません", cx);
                            });
                        }
                    });
                }),
            )
    }

    fn scene_argument_details(
        &self,
        argument: &SceneArgumentOption,
        render: &RenderCtx<'_>,
    ) -> gpui::AnyElement {
        if argument.derived {
            return self
                .store
                .states
                .get(&ControlId::scene_argument_expression(
                    argument.scene_id,
                    &argument.id,
                ))
                .and_then(state::ControlState::text)
                .map(|state| Self::scene_text_input_row("式", state.input.clone()))
                .map(IntoElement::into_any_element)
                .unwrap_or_else(|| div().into_any_element());
        }

        let mut details = div().w_full().flex().flex_col().gap_2();
        if let Some(settings) = self.numeric_scene_argument_settings(argument, render) {
            details = details.child(settings);
        }
        if let Some(input) = self
            .store
            .states
            .get(&ControlId::scene_argument_default(
                argument.scene_id,
                &argument.id,
            ))
            .and_then(state::ControlState::text)
        {
            details = details.child(Self::scene_text_input_row(
                "デフォルト",
                input.input.clone(),
            ));
        }
        if let Some(picker) = self
            .store
            .states
            .get(&ControlId::scene_argument_color(
                argument.scene_id,
                &argument.id,
            ))
            .and_then(state::ControlState::color)
        {
            details = details.child(Self::scene_color_row(picker.picker.clone()));
        }
        if let ParameterValue::Bool(checked) = argument.schema.default_value() {
            details = details.child(Self::scene_bool_row(
                argument.id.clone(),
                *checked,
                render.editor,
            ));
        }
        details.into_any_element()
    }

    fn numeric_scene_argument_settings(
        &self,
        argument: &SceneArgumentOption,
        render: &RenderCtx<'_>,
    ) -> Option<gpui::AnyElement> {
        let input = |setting| {
            self.store
                .states
                .get(&ControlId::scene_argument_setting(
                    argument.scene_id,
                    &argument.id,
                    setting,
                ))
                .and_then(state::ControlState::text)
                .map(|state| state.input.clone())
        };
        let default = input(SceneArgumentSetting::Default)?;
        let min = input(SceneArgumentSetting::Min)?;
        let max = input(SceneArgumentSetting::Max)?;

        Some(
            div()
                .w_full()
                .flex()
                .flex_col()
                .gap_2()
                .child(Self::scene_argument_setting_row(
                    "デフォルト",
                    &default,
                    argument.scene_id,
                    &argument.id,
                    SceneArgumentSetting::Default,
                    &render.inspector,
                    render.focus_handle,
                ))
                .child(Self::scene_argument_setting_row(
                    "最小",
                    &min,
                    argument.scene_id,
                    &argument.id,
                    SceneArgumentSetting::Min,
                    &render.inspector,
                    render.focus_handle,
                ))
                .child(Self::scene_argument_setting_row(
                    "最大",
                    &max,
                    argument.scene_id,
                    &argument.id,
                    SceneArgumentSetting::Max,
                    &render.inspector,
                    render.focus_handle,
                ))
                .into_any_element(),
        )
    }

    fn scene_argument_setting_row(
        label: &'static str,
        input: &Entity<InputState>,
        scene_id: SceneId,
        argument_id: &str,
        setting: SceneArgumentSetting,
        inspector: &Entity<Self>,
        focus_handle: &FocusHandle,
    ) -> gpui::AnyElement {
        let drag = SceneArgumentValueDrag {
            inspector_id: inspector.entity_id(),
            scene_id,
            argument_id: argument_id.to_owned(),
            setting,
        };
        let drag_origin = drag.clone();
        let drag_inspector = inspector.clone();
        let drag_input = input.clone();
        let drag_focus_handle = focus_handle.clone();
        div()
            .w_full()
            .flex()
            .items_center()
            .gap_3()
            .child(Self::parameter_label_column(label))
            .child(
                div()
                    .id(SharedString::from(format!(
                        "scene-argument-setting-drag-{scene_id:?}-{argument_id}-{setting:?}"
                    )))
                    .w_0()
                    .min_w_0()
                    .flex()
                    .flex_1()
                    .on_mouse_down(MouseButton::Left, move |event, _, cx| {
                        drag_inspector.update(cx, |inspector, cx| {
                            inspector.prepare_scene_argument_value_drag(&drag_origin, event, cx);
                        });
                    })
                    .on_drag(drag, move |drag, _, window, cx| {
                        cx.stop_propagation();
                        drag_input.update(cx, |input, cx| input.unselect(window, cx));
                        drag_focus_handle.focus(window, cx);
                        cx.new(|_| drag.clone())
                    })
                    .child(NumberInput::new(input).small().w_full()),
            )
            .into_any_element()
    }

    fn scene_text_input_row(label: &'static str, input: Entity<InputState>) -> Div {
        div()
            .w_full()
            .min_w_0()
            .flex()
            .items_center()
            .gap_3()
            .child(Self::parameter_label_column(label))
            .child(
                div()
                    .w_0()
                    .min_w_0()
                    .flex_1()
                    .child(Input::new(&input).small().w_full()),
            )
    }

    fn scene_color_row(picker: Entity<ColorPickerState>) -> Div {
        div()
            .w_full()
            .flex()
            .items_center()
            .gap_3()
            .child(Self::parameter_label_column("デフォルト"))
            .child(
                div()
                    .w_0()
                    .min_w_0()
                    .flex_1()
                    .child(ColorPicker::new(&picker).small().w_full()),
            )
    }

    fn scene_bool_row(argument_id: String, checked: bool, editor: &Entity<TimelineEditor>) -> Div {
        let editor = editor.clone();
        let switch_id = argument_id.clone();
        div()
            .w_full()
            .flex()
            .items_center()
            .gap_3()
            .child(Self::parameter_label_column("デフォルト"))
            .child(
                Switch::new(SharedString::from(format!(
                    "scene-argument-default-{switch_id}"
                )))
                .small()
                .checked(checked)
                .on_click(move |checked, _, cx| {
                    editor.update(cx, |editor, cx| {
                        if editor.update_scene_argument_default(
                            &argument_id,
                            ParameterValue::Bool(*checked),
                        ) {
                            cx.notify();
                        }
                    });
                }),
            )
    }
}

fn numeric_settings(number: &NumericInput, schema: &ParameterSchema) -> Option<[String; 3]> {
    let default = match schema.default_value() {
        ParameterValue::F32(value) => f64::from(*value),
        ParameterValue::I32(value) => f64::from(*value),
        ParameterValue::U32(value) => f64::from(*value),
        _ => return None,
    };
    let constraints = schema.constraints();
    let (type_min, type_max) = number.bounds();
    let bound = |value: Option<f64>, lower: bool| {
        value
            .map(|value| {
                let value = match number.scalar {
                    ScalarParameterType::I32 | ScalarParameterType::U32 => {
                        if lower {
                            value.ceil().max(type_min)
                        } else {
                            value.floor().min(type_max)
                        }
                    }
                    _ => value,
                };
                number.format(value * number.scale)
            })
            .unwrap_or_default()
    };
    Some([
        number.format(default * number.scale),
        bound(constraints.min, true),
        bound(constraints.max, false),
    ])
}

fn setting_display_value(
    number: &NumericInput,
    setting: SceneArgumentSetting,
    text: &str,
) -> Option<f64> {
    if let Some(value) = number.parse_optional(text)? {
        return value.numeric_scalar().map(|value| value * number.scale);
    }
    let (min, max) = number.bounds();
    match setting {
        SceneArgumentSetting::Default => None,
        SceneArgumentSetting::Min => Some(min * number.scale),
        SceneArgumentSetting::Max => Some(max * number.scale),
    }
}

fn clamp_setting(
    number: &NumericInput,
    setting: SceneArgumentSetting,
    value: f64,
    min: f64,
    max: f64,
) -> Option<f64> {
    if !value.is_finite() || !min.is_finite() || !max.is_finite() || min > max {
        return None;
    }
    let (lower, upper) = number.bounds();
    let value = value.clamp(lower * number.scale, upper * number.scale);
    Some(match setting {
        SceneArgumentSetting::Default => value.clamp(min, max),
        SceneArgumentSetting::Min => value.min(max),
        SceneArgumentSetting::Max => value.max(min),
    })
}
