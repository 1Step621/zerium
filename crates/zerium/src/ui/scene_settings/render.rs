use super::*;

#[derive(Clone)]
pub(super) struct SceneVariableOption {
    pub(super) scene_id: SceneId,
    pub(super) id: String,
    pub(super) schema: Option<PropertySchema>,
    pub(super) label: String,
    pub(super) output_type: zerium_core::property::ScalarPropertyType,
    pub(super) binding_count: usize,
    pub(super) calculation: Option<SceneCalculation>,
    pub(super) result: Option<Result<PropertyValue, String>>,
}

pub(super) struct SceneSettingsRenderCtx<'a> {
    pub(super) colors: ThemeColor,
    pub(super) editor: &'a Entity<TimelineEditor>,
    pub(super) settings: Entity<SceneSettings>,
    pub(super) active_scene_name_input: Option<Entity<InputState>>,
    pub(super) file_input: Entity<FileInputController>,
    pub(super) selecting_file: bool,
    pub(super) sources: Vec<(String, String)>,
}

impl Render for SceneSettings {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let args = self
            .editor
            .read(cx)
            .active_scene_id()
            .and_then(|id| {
                self.editor.read(cx).scene(id).map(|scene| {
                    let evaluation = scene.evaluate_variables(None);
                    scene
                        .variables()
                        .iter()
                        .map(|arg| SceneVariableOption {
                            scene_id: id,
                            id: arg.id().to_owned(),
                            schema: arg.argument_schema().cloned(),
                            label: arg.label().to_owned(),
                            output_type: arg.ty().clone(),
                            binding_count: arg.bindings.len(),
                            calculation: arg.calculation().cloned(),
                            result: evaluation
                                .values
                                .get(arg.id())
                                .cloned()
                                .map(|value| value.map_err(|error| error.to_string())),
                        })
                        .collect::<Vec<_>>()
                })
            })
            .unwrap_or_default();
        let render = SceneSettingsRenderCtx {
            colors: cx.theme().colors,
            sources: args
                .iter()
                .map(|variable| (variable.id.clone(), variable.label.clone()))
                .collect(),
            editor: &self.editor,
            settings: cx.entity(),
            file_input: self.file_input.clone(),
            selecting_file: self.file_input.read(cx).is_selecting(),
            active_scene_name_input: self.scene_id.and_then(|id| {
                self.store
                    .inputs
                    .text_inputs
                    .get(&ControlId::scene_name(id))
                    .map(|state| state.input.clone())
            }),
        };
        div()
            .id("scene-settings")
            .size_full()
            .flex()
            .flex_col()
            .overflow_hidden()
            .bg(render.colors.background)
            .track_focus(&self.focus_handle)
            .on_drag_move(cx.listener(Self::handle_number_drag))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| this.finish_number_drag(cx)),
            )
            .on_mouse_up_out(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| this.finish_number_drag(cx)),
            )
            .child(
                div()
                    .id("scene-settings-scroll")
                    .w_full()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .flex_col()
                    .overflow_y_scroll()
                    .track_scroll(&self.scroll_handle)
                    .gap_3()
                    .p_3()
                    .when(self.scene_id.is_some(), |this| {
                        this.children(self.scene_settings_elements(&args, &render))
                    })
                    .when(self.scene_id.is_none(), |this| {
                        this.child(
                            div()
                                .text_sm()
                                .text_color(render.colors.muted_foreground)
                                .child(t!("args.no_active_scene").to_string()),
                        )
                    }),
            )
    }
}

impl SceneSettings {
    fn scene_settings_elements(
        &self,
        arguments: &[SceneVariableOption],
        render: &SceneSettingsRenderCtx<'_>,
    ) -> Vec<gpui::AnyElement> {
        let active_scene_name_input = render.active_scene_name_input.clone();
        let input_arguments = arguments
            .iter()
            .filter(|variable| variable.calculation.is_none())
            .cloned()
            .collect::<Vec<_>>();
        let calculations = arguments
            .iter()
            .filter(|variable| variable.calculation.is_some())
            .cloned()
            .collect::<Vec<_>>();
        let rows = input_arguments
            .iter()
            .cloned()
            .enumerate()
            .map(|(index, argument)| {
                self.scene_argument_element(index, input_arguments.len(), argument, render)
            })
            .collect::<Vec<_>>();

        let mut elements = Vec::new();
        if let Some(input) = active_scene_name_input {
            elements.push(
                scene_text_input_row(t!("args.scene_name").to_string(), input)
                    .flex_none()
                    .into_any_element(),
            );
        }
        elements.push(
            scene_variables_header(render.editor, false)
                .flex_none()
                .into_any_element(),
        );
        elements.extend(rows);
        elements.push(
            scene_variables_header(render.editor, true)
                .flex_none()
                .into_any_element(),
        );
        elements.extend(
            calculations
                .iter()
                .cloned()
                .enumerate()
                .map(|(index, variable)| {
                    self.scene_argument_element(index, calculations.len(), variable, render)
                }),
        );
        elements
    }

    fn scene_argument_element(
        &self,
        index: usize,
        count: usize,
        argument: SceneVariableOption,
        render: &SceneSettingsRenderCtx<'_>,
    ) -> gpui::AnyElement {
        let name_input = self
            .store
            .inputs
            .text_inputs
            .get(&ControlId::scene_argument_name(
                argument.scene_id,
                &argument.id,
            ))
            .map(|state| state.input.clone());

        div()
            .w_full()
            .min_w_0()
            .flex()
            .flex_col()
            .flex_none()
            .gap_2()
            .child(scene_argument_header(
                index, count, &argument, name_input, render,
            ))
            .child(self.scene_argument_details(&argument, render))
            .into_any_element()
    }

    fn scene_argument_details(
        &self,
        argument: &SceneVariableOption,
        render: &SceneSettingsRenderCtx<'_>,
    ) -> gpui::AnyElement {
        if argument.calculation.is_some() {
            return self
                .calculation_details(argument, render)
                .into_any_element();
        }
        let schema = argument.schema.as_ref().expect("argument row");
        let mut details = div().w_full().flex().flex_col().gap_2();
        if let Some(settings) = self.numeric_scene_argument_settings(argument, render) {
            details = details.child(settings);
            if let Some(Ok(applied)) = &argument.result
                && applied != &schema.default_value()
                && let Some(value) = applied.numeric_text()
            {
                details = details.child(
                    div()
                        .text_sm()
                        .text_color(render.colors.muted_foreground)
                        .child(t!("args.applied_value", value = value).to_string()),
                );
            }
        }
        if let Some(input) = self
            .store
            .inputs
            .text_inputs
            .get(&ControlId::scene_argument_default(
                argument.scene_id,
                &argument.id,
            ))
        {
            details = details.child(scene_text_input_row(
                t!("args.default").to_string(),
                input.input.clone(),
            ));
        }
        if let Some(picker) = self
            .store
            .inputs
            .color_pickers
            .get(&ControlId::scene_argument_color(
                argument.scene_id,
                &argument.id,
            ))
        {
            details = details.child(scene_color_row(picker.picker.clone()));
        }
        if let PropertyValue::Bool(checked) = schema.default_value() {
            details = details.child(scene_bool_row(argument.id.clone(), checked, render.editor));
        }
        if let PropertyValue::File(file) = schema.default_value() {
            details = details.child(crate::ui::file_input::file_picker(
                &render.file_input,
                SharedString::from(format!("argument-file-{}", argument.id)),
                FileTarget::Argument {
                    scene_id: argument.scene_id,
                    argument_id: argument.id.clone(),
                },
                file.as_deref(),
                render.selecting_file,
                false,
            ));
        }
        details.into_any_element()
    }

    fn numeric_scene_argument_settings(
        &self,
        argument: &SceneVariableOption,
        render: &SceneSettingsRenderCtx<'_>,
    ) -> Option<gpui::AnyElement> {
        let rows = [
            (NumericSetting::Default, t!("args.default")),
            (NumericSetting::Min, t!("args.minimum")),
            (NumericSetting::Max, t!("args.maximum")),
        ]
        .into_iter()
        .map(|(setting, label)| {
            let input = self.store.inputs.text(&ControlId::scene_argument_setting(
                argument.scene_id,
                &argument.id,
                setting,
            ))?;
            Some(scene_argument_setting_row(
                label.to_string(),
                &input,
                argument.scene_id,
                &argument.id,
                setting,
                &render.settings,
            ))
        })
        .collect::<Option<Vec<_>>>()?;
        Some(
            div()
                .w_full()
                .flex()
                .flex_col()
                .gap_2()
                .children(rows)
                .into_any_element(),
        )
    }
}

fn property_label_column(label: impl Into<SharedString>) -> Div {
    div()
        .w(px(90.))
        .min_h(px(24.))
        .flex_none()
        .flex()
        .items_center()
        .child(
            div()
                .w_full()
                .text_sm()
                .whitespace_normal()
                .child(label.into()),
        )
}

fn scene_variables_header(editor: &Entity<TimelineEditor>, calculation: bool) -> Div {
    let create_editor = editor.clone();
    div()
        .w_full()
        .flex()
        .items_center()
        .justify_between()
        .child(div().text_sm().child(if calculation {
            t!("calc.title").to_string()
        } else {
            t!("args.scene_arguments").to_string()
        }))
        .child(
            Button::new(if calculation {
                "create-scene-calculation"
            } else {
                "create-scene-argument"
            })
            .small()
            .compact()
            .label(if calculation {
                t!("calc.create").to_string()
            } else {
                t!("args.create").to_string()
            })
            .dropdown_caret(true)
            .popup_menu(move |menu, _, _| {
                [
                    (t!("args.number").to_string(), SceneVariablePreset::Number),
                    (
                        t!("args.integer").to_string(),
                        SceneVariablePreset::SignedInteger,
                    ),
                    (
                        t!("args.unsigned_integer").to_string(),
                        SceneVariablePreset::UnsignedInteger,
                    ),
                    (t!("args.boolean").to_string(), SceneVariablePreset::Boolean),
                    (t!("args.color").to_string(), SceneVariablePreset::Color),
                    (t!("args.string").to_string(), SceneVariablePreset::Text),
                    (t!("args.file").to_string(), SceneVariablePreset::File),
                ]
                .into_iter()
                .filter(|(_, preset)| !calculation || *preset != SceneVariablePreset::File)
                .fold(menu, |menu, (label, ty)| {
                    let editor = create_editor.clone();
                    menu.item(PopupMenuItem::new(label).on_click(move |_, _, cx| {
                        editor.update(cx, |editor, cx| {
                            let created = if calculation {
                                editor.create_scene_calculation(ty, |ordinal| {
                                    t!("calc.new", number = ordinal).to_string()
                                })
                            } else {
                                editor.create_scene_argument(ty, |ordinal| {
                                    t!("args.new_argument", number = ordinal).to_string()
                                })
                            };
                            if created.is_some() {
                                cx.notify();
                            }
                        });
                    }))
                })
            }),
        )
}

fn scene_argument_header(
    index: usize,
    count: usize,
    argument: &SceneVariableOption,
    name_input: Option<Entity<InputState>>,
    render: &SceneSettingsRenderCtx<'_>,
) -> Div {
    let move_up_editor = render.editor.clone();
    let move_up_id = argument.id.clone();
    let move_down_editor = render.editor.clone();
    let move_down_id = argument.id.clone();
    let remove_settings = render.settings.clone();
    let remove_id = argument.id.clone();

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
                .child(t!("args.binding_count", count = argument.binding_count).to_string()),
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
            .tooltip(t!("common.move_up").to_string())
            .disabled(index == 0)
            .on_click(move |_, _, cx| {
                move_up_editor.update(cx, |editor, cx| {
                    if editor.move_scene_variable(&move_up_id, -1) {
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
            .tooltip(t!("common.move_down").to_string())
            .disabled(index + 1 == count)
            .on_click(move |_, _, cx| {
                move_down_editor.update(cx, |editor, cx| {
                    if editor.move_scene_variable(&move_down_id, 1) {
                        cx.notify();
                    }
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
            .tooltip(t!("args.remove").to_string())
            .on_click(move |_, _, cx| {
                remove_settings.update(cx, |settings, cx| {
                    let result = settings.editor.update(cx, |editor, cx| {
                        let result = editor.remove_scene_variable(&remove_id);
                        if result.is_ok() {
                            cx.notify();
                        }
                        result
                    });
                    if let Err(error) = result {
                        settings.notifications.update(cx, |notifications, cx| {
                            notifications
                                .push(format!("{}: {error}", t!("args.remove_failed")), cx);
                        });
                    }
                });
            }),
        )
}

fn scene_argument_setting_row(
    label: String,
    input: &Entity<InputState>,
    scene_id: SceneId,
    argument_id: &str,
    setting: NumericSetting,
    settings: &Entity<SceneSettings>,
) -> gpui::AnyElement {
    let argument_id = argument_id.to_owned();
    div()
        .w_full()
        .flex()
        .items_center()
        .gap_3()
        .child(property_label_column(label))
        .child(crate::ui::number_input::number_input_drag(
            settings,
            input,
            NumberInput::new(input).small().w_full(),
            false,
            move |this, event, cx| {
                this.prepare_scene_argument_value_drag(scene_id, &argument_id, setting, event, cx)
            },
        ))
        .into_any_element()
}

fn scene_text_input_row(label: String, input: Entity<InputState>) -> Div {
    div()
        .w_full()
        .min_w_0()
        .flex()
        .items_center()
        .gap_3()
        .child(property_label_column(label))
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
        .child(property_label_column(t!("args.default").to_string()))
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
        .child(property_label_column(t!("args.default").to_string()))
        .child(
            Switch::new(SharedString::from(format!(
                "scene-argument-default-{switch_id}"
            )))
            .small()
            .checked(checked)
            .on_click(move |checked, _, cx| {
                editor.update(cx, |editor, cx| {
                    if editor
                        .update_scene_argument_default(&argument_id, PropertyValue::Bool(*checked))
                        == Ok(true)
                    {
                        cx.notify();
                    }
                });
            }),
        )
}
