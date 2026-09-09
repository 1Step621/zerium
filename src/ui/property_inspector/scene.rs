use super::*;

impl PropertyInspector {
    pub(super) fn array_has_scene_binding(
        item_id: ItemId,
        owner: SceneBindingOwner,
        parameter_id: &str,
        arguments: &[SceneArgumentOption],
    ) -> bool {
        arguments.iter().any(|argument| {
            argument.bindings.iter().any(|binding| {
                binding.item_id() == item_id
                    && binding.owner() == owner
                    && binding.parameter_id() == parameter_id
                    && binding.value_path().array_element().is_some()
            })
        })
    }

    pub(super) fn scene_binding_for_property(
        editing_scene: bool,
        animation_enabled: bool,
        item_id: ItemId,
        field: &NumberField,
        arguments: &[SceneArgumentOption],
    ) -> Option<SceneFieldBinding> {
        let tuple_element = field.target.value_path.tuple_element();
        let ty = ParameterType::Value(ParameterValueType::Scalar(field.scalar_type.clone()));
        Self::scene_field_binding(
            editing_scene,
            animation_enabled,
            field.scene_bindable,
            SceneBindingTarget::new(
                item_id,
                SceneBindingOwner::from_effect(field.target.effect_id),
                field.target.parameter_id.clone(),
                SceneBindingValuePath::from_elements(
                    field.target.value_path.array_element(),
                    tuple_element,
                ),
            ),
            &ty,
            arguments,
        )
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
        let key = (scene_id, argument_id.to_owned(), setting);
        if let Some(input) = self.controls.scene_argument_setting_inputs.get(&key) {
            Self::set_input_value(input, value, window, cx);
            return;
        }
        let input =
            cx.new(|cx| InputState::new(window, cx).default_value(SharedString::from(value)));
        let step_argument_id = argument_id.to_owned();
        self.controls.input_subscriptions.push(cx.subscribe_in(
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
        ));
        let argument_id = argument_id.to_owned();
        self.controls.input_subscriptions.push(cx.subscribe_in(
            &input,
            window,
            move |this, _, event, _, cx| {
                if matches!(event, InputEvent::Change) {
                    this.apply_scene_argument_settings(scene_id, &argument_id, cx);
                }
            },
        ));
        self.controls
            .scene_argument_setting_inputs
            .insert(key, input);
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
        let parse = |setting| {
            let text = self
                .controls
                .scene_argument_setting_inputs
                .get(&(scene_id, argument_id.to_owned(), setting))?
                .read(cx)
                .value();
            setting_display_value(&number, setting, &text)
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
            self.controls
                .scene_argument_setting_inputs
                .get(&(scene_id, argument_id.to_owned(), setting))
                .map(|input| input.read(cx).value().to_string())
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
        if let Some(input) = self.controls.scene_name_inputs.get(&scene_id) {
            Self::set_input_value(input, scene_name, window, cx);
        } else {
            let input = cx.new(|cx| {
                InputState::new(window, cx).default_value(SharedString::from(scene_name))
            });
            self.controls.input_subscriptions.push(cx.subscribe_in(
                &input,
                window,
                move |this, input, event, _, cx| {
                    if !matches!(event, InputEvent::Change) {
                        return;
                    }
                    let name = input.read(cx).value().to_string();
                    this.editor.update(cx, |editor, cx| {
                        if editor.rename_scene(scene_id, name) {
                            cx.notify();
                        }
                    });
                },
            ));
            self.controls.scene_name_inputs.insert(scene_id, input);
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
            let key = (scene_id, argument.schema.id().to_owned());
            let label = if argument.schema.label().is_empty() {
                argument.schema.id().to_owned()
            } else {
                argument.schema.label().to_owned()
            };
            if let Some(input) = self.controls.scene_argument_name_inputs.get(&key) {
                Self::set_input_value(input, label, window, cx);
            } else {
                let input = cx
                    .new(|cx| InputState::new(window, cx).default_value(SharedString::from(label)));
                let argument_id = argument.schema.id().to_owned();
                self.controls.input_subscriptions.push(cx.subscribe_in(
                    &input,
                    window,
                    move |this, input, event, _, cx| {
                        if !matches!(event, InputEvent::Change) {
                            return;
                        }
                        let label = input.read(cx).value().to_string();
                        this.editor.update(cx, |editor, cx| {
                            if editor.rename_scene_argument(&argument_id, &label) {
                                cx.notify();
                            }
                        });
                    },
                ));
                self.controls
                    .scene_argument_name_inputs
                    .insert(key.clone(), input);
            }

            if let Some(expression) = displayed_expressions.get(argument.schema.id()).cloned() {
                if let Some(input) = self.controls.scene_argument_expression_inputs.get(&key) {
                    Self::set_input_value(input, expression, window, cx);
                } else {
                    let input = cx.new(|cx| {
                        InputState::new(window, cx).default_value(SharedString::from(expression))
                    });
                    let argument_id = argument.schema.id().to_owned();
                    self.controls.input_subscriptions.push(cx.subscribe_in(
                        &input,
                        window,
                        move |this, input, event, _, cx| {
                            if !matches!(event, InputEvent::Change) {
                                return;
                            }
                            let expression = input.read(cx).value().to_string();
                            this.editor.update(cx, |editor, cx| {
                                if editor
                                    .update_scene_argument_expression(&argument_id, &expression)
                                {
                                    cx.notify();
                                }
                            });
                        },
                    ));
                    self.controls
                        .scene_argument_expression_inputs
                        .insert(key.clone(), input);
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
                        argument.schema.id(),
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
                if let Some(input) = self.controls.scene_argument_default_inputs.get(&key) {
                    Self::set_input_value(input, value, window, cx);
                } else {
                    let input = cx.new(|cx| {
                        InputState::new(window, cx).default_value(SharedString::from(value))
                    });
                    let argument_id = argument.schema.id().to_owned();
                    self.controls.input_subscriptions.push(cx.subscribe_in(
                        &input,
                        window,
                        move |this, input, event, _, cx| {
                            if !matches!(event, InputEvent::Change) {
                                return;
                            }
                            let value = input.read(cx).value().to_string();
                            this.editor.update(cx, |editor, cx| {
                                if editor.update_scene_argument_default(
                                    &argument_id,
                                    ParameterValue::String(value),
                                ) {
                                    cx.notify();
                                }
                            });
                        },
                    ));
                    self.controls
                        .scene_argument_default_inputs
                        .insert(key, input);
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
                if let Some(picker) = self.controls.scene_argument_color_pickers.get(&key) {
                    if picker.read(cx).value() != Some(color) {
                        picker.update(cx, |picker, cx| picker.set_value(color, window, cx));
                    }
                } else {
                    let picker =
                        cx.new(|cx| ColorPickerState::new(window, cx).default_value(color));
                    let argument_id = argument.schema.id().to_owned();
                    self.controls.input_subscriptions.push(cx.subscribe_in(
                        &picker,
                        window,
                        move |this, _, event, _, cx| {
                            let ColorPickerEvent::Change(Some(color)) = event else {
                                return;
                            };
                            let color = Rgba::from(*color);
                            let value = ParameterValue::Color([color.r, color.g, color.b, color.a]);
                            this.editor.update(cx, |editor, cx| {
                                if editor.update_scene_argument_default(&argument_id, value) {
                                    cx.notify();
                                }
                            });
                        },
                    ));
                    self.controls
                        .scene_argument_color_pickers
                        .insert(key, picker);
                }
            }
        }
    }

    pub(super) fn ensure_scene_inputs(
        &mut self,
        item: &TimelineItem,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(scene_id) = item.scene_id() else {
            return;
        };
        let arguments = self
            .editor
            .read(cx)
            .scene(scene_id)
            .map(|scene| scene.arguments.clone())
            .unwrap_or_default();
        self.ensure_numeric_inputs(
            item,
            Self::scene_property_fields(scene_id, &arguments),
            window,
            cx,
        );
        self.ensure_color_inputs(
            item,
            Self::scene_color_fields(scene_id, &arguments),
            window,
            cx,
        );
        let strings =
            Self::string_fields(Self::scene_property_controls(scene_id, &arguments, item));
        self.ensure_string_inputs(item, strings, window, cx);
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
        let input = self.controls.scene_argument_setting_inputs.get(&(
            drag.scene_id,
            drag.argument_id.clone(),
            drag.setting,
        ));
        let Some(start_value) = input.and_then(|input| {
            setting_display_value(&number, drag.setting, &input.read(cx).value())
        }) else {
            return;
        };
        let parse = |setting| {
            let text = self
                .controls
                .scene_argument_setting_inputs
                .get(&(drag.scene_id, drag.argument_id.clone(), setting))?
                .read(cx)
                .value();
            setting_display_value(&number, setting, &text)
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
        self.controls.scene_argument_value_drag_origin = Some(SceneArgumentValueDragOrigin {
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
            .controls
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
            let text = self
                .controls
                .scene_argument_setting_inputs
                .get(&(drag.scene_id, drag.argument_id.clone(), setting))?
                .read(cx)
                .value();
            setting_display_value(&origin.number, setting, &text)
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
        let Some(input) = self.controls.scene_argument_setting_inputs.get(&(
            drag.scene_id,
            drag.argument_id.clone(),
            drag.setting,
        )) else {
            return;
        };
        Self::set_input_value(input, origin.number.format(value), window, cx);
        self.apply_scene_argument_settings(drag.scene_id, &drag.argument_id, cx);
    }

    pub(super) fn scene_argument_setting_row(
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

    pub(super) fn scene_field_binding(
        editing_scene: bool,
        animation_enabled: bool,
        scene_bindable: bool,
        target: SceneBindingTarget,
        ty: &ParameterType,
        arguments: &[SceneArgumentOption],
    ) -> Option<SceneFieldBinding> {
        if !editing_scene || animation_enabled || !scene_bindable {
            return None;
        }
        let connected = arguments.iter().find_map(|argument| {
            argument
                .bindings
                .contains(&target)
                .then(|| (argument.id.clone(), argument.label.clone()))
        });
        let compatible = arguments
            .iter()
            .filter(|argument| argument.schema.ty() == ty)
            .map(|argument| (argument.id.clone(), argument.label.clone()))
            .collect();
        Some(SceneFieldBinding {
            target,
            connected,
            compatible,
        })
    }

    pub(super) fn scene_binding_button(
        binding: SceneFieldBinding,
        inspector: &Entity<Self>,
        key: SharedString,
    ) -> gpui::AnyElement {
        let menu_inspector = inspector.clone();
        let button_label = binding
            .connected
            .as_ref()
            .map_or_else(|| "引数".to_owned(), |(_, label)| format!("→ {label}"));
        Button::new(key)
            .small()
            .compact()
            .ghost()
            .label(button_label)
            .dropdown_caret(true)
            .tooltip("シーン引数との接続を設定")
            .popup_menu(move |menu, _, _| {
                if let Some((argument_id, _)) = &binding.connected {
                    let inspector = menu_inspector.clone();
                    let argument_id = argument_id.clone();
                    let target = binding.target.clone();
                    return menu.item(PopupMenuItem::new("接続を解除").on_click(
                        move |_, _, cx| {
                            inspector.update(cx, |inspector, cx| {
                                let result = inspector.editor.update(cx, |editor, cx| {
                                    let result =
                                        editor.disconnect_scene_argument(&argument_id, &target);
                                    if result.is_ok() {
                                        cx.notify();
                                    }
                                    result
                                });
                                if result.is_err() {
                                    inspector.notifications.update(cx, |notifications, cx| {
                                        notifications.push("シーン引数の接続を解除できません", cx);
                                    });
                                }
                            });
                        },
                    ));
                }

                let inspector = menu_inspector.clone();
                let target = binding.target.clone();
                binding.compatible.iter().fold(
                    menu.item(PopupMenuItem::new("この値から新しい引数を作成").on_click(
                        move |_, _, cx| {
                            inspector.update(cx, |inspector, cx| {
                                let result = inspector.editor.update(cx, |editor, cx| {
                                    let result = editor.add_scene_argument(target.clone());
                                    if result.is_ok() {
                                        cx.notify();
                                    }
                                    result
                                });
                                if result.is_err() {
                                    inspector.notifications.update(cx, |notifications, cx| {
                                        notifications.push("シーン引数を作成できません", cx);
                                    });
                                }
                            });
                        },
                    )),
                    |menu, (argument_id, label)| {
                        let inspector = menu_inspector.clone();
                        let argument_id = argument_id.clone();
                        let target = binding.target.clone();
                        menu.item(PopupMenuItem::new(label.clone()).on_click(move |_, _, cx| {
                            inspector.update(cx, |inspector, cx| {
                                let result = inspector.editor.update(cx, |editor, cx| {
                                    let result =
                                        editor.connect_scene_argument(&argument_id, target.clone());
                                    if result.is_ok() {
                                        cx.notify();
                                    }
                                    result
                                });
                                if result.is_err() {
                                    inspector.notifications.update(cx, |notifications, cx| {
                                        notifications.push("シーン引数へ接続できません", cx);
                                    });
                                }
                            });
                        }))
                    },
                )
            })
            .into_any_element()
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
