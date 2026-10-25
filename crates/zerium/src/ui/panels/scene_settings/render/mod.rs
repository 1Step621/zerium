mod calculation;
use super::*;
use zerium_core::property::ScalarPropertyType;

pub(super) struct SceneVariableRow {
    pub(super) scene_id: SceneId,
    pub(super) variable: zerium_core::timeline::SceneVariable,
    pub(super) result: Option<Result<PropertyValue, String>>,
}

pub(super) struct SceneSettingsRenderCtx<'a> {
    pub(super) colors: ThemeColor,
    pub(super) is_main_scene: bool,
    pub(super) editor: &'a Entity<TimelineEditor>,
    pub(super) settings: Entity<SceneSettings>,
    pub(super) active_scene_name_input: Option<Entity<InputState>>,
    pub(super) file_input: Entity<FileInputController>,
    pub(super) selecting_file: bool,
    pub(super) sources: Vec<(String, String)>,
}

impl SceneSettingsRenderCtx<'_> {
    fn argument_value_label(&self) -> String {
        if self.is_main_scene {
            t!("args.value").to_string()
        } else {
            t!("args.default").to_string()
        }
    }
}

impl Render for SceneSettings {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let editor = self.editor.read(cx);
        let scene = editor.active_scene();
        let evaluation = scene.evaluate_default_variables();
        let args = scene
            .variables()
            .iter()
            .map(|variable| SceneVariableRow {
                scene_id: scene.id,
                variable: variable.clone(),
                result: evaluation
                    .values
                    .get(variable.id())
                    .cloned()
                    .map(|value| value.map_err(|error| error.to_string())),
            })
            .collect::<Vec<_>>();
        let render = SceneSettingsRenderCtx {
            colors: cx.theme().colors,
            is_main_scene: scene.id == editor.main_scene_id(),
            sources: args
                .iter()
                .map(|row| {
                    (
                        row.variable.id().to_owned(),
                        row.variable.label().to_owned(),
                    )
                })
                .collect(),
            editor: &self.editor,
            settings: cx.entity(),
            file_input: self.file_input.clone(),
            selecting_file: self.file_input.read(cx).is_selecting(),
            active_scene_name_input: self
                .store
                .inputs
                .text(&ControlId::scene_name(self.scene_id)),
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
                    .gap_4()
                    .p_3()
                    .children(self.scene_settings_elements(&args, &render)),
            )
    }
}

impl SceneSettings {
    fn scene_settings_elements(
        &self,
        arguments: &[SceneVariableRow],
        render: &SceneSettingsRenderCtx<'_>,
    ) -> Vec<gpui::AnyElement> {
        let active_scene_name_input = render.active_scene_name_input.clone();
        let (input_arguments, calculations): (Vec<_>, Vec<_>) = arguments
            .iter()
            .partition(|row| row.variable.calculation().is_none());
        let mut elements = Vec::new();
        if let Some(input) = active_scene_name_input {
            elements.push(
                scene_value_row(
                    t!("args.scene_name").to_string(),
                    Input::new(&input).small().w_full(),
                )
                .flex_none()
                .into_any_element(),
            );
        }
        elements.push(
            self.scene_variables_section(&input_arguments, false, render)
                .into_any_element(),
        );
        elements.push(
            self.scene_variables_section(&calculations, true, render)
                .into_any_element(),
        );
        elements
    }

    fn scene_variables_section(
        &self,
        variables: &[&SceneVariableRow],
        calculation: bool,
        render: &SceneSettingsRenderCtx<'_>,
    ) -> Div {
        div()
            .w_full()
            .min_w_0()
            .flex_none()
            .flex()
            .flex_col()
            .border_1()
            .border_color(render.colors.border)
            .rounded_md()
            .child(
                div()
                    .w_full()
                    .flex_none()
                    .px_3()
                    .py_2()
                    .rounded_t_md()
                    .bg(render.colors.list_head)
                    .border_b_1()
                    .border_color(render.colors.border)
                    .text_sm()
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .child(if calculation {
                        t!("calc.title").to_string()
                    } else {
                        t!("args.scene_arguments").to_string()
                    }),
            )
            .children(variables.iter().enumerate().map(|(index, variable)| {
                div()
                    .w_full()
                    .min_w_0()
                    .flex_none()
                    .p_3()
                    .border_b_1()
                    .border_color(render.colors.border)
                    .child(self.scene_argument_element(index, variables.len(), variable, render))
            }))
            .child(
                div()
                    .w_full()
                    .flex_none()
                    .flex()
                    .p_3()
                    .child(scene_variable_add_button(render.editor, calculation)),
            )
    }

    fn scene_argument_element(
        &self,
        index: usize,
        count: usize,
        argument: &SceneVariableRow,
        render: &SceneSettingsRenderCtx<'_>,
    ) -> gpui::AnyElement {
        let name_input = self.store.inputs.text(&ControlId::scene_argument_name(
            argument.scene_id,
            argument.variable.id(),
        ));

        div()
            .w_full()
            .min_w_0()
            .flex()
            .flex_col()
            .flex_none()
            .gap_2()
            .child(scene_argument_header(
                index, count, argument, name_input, render,
            ))
            .child(self.scene_argument_details(argument, render))
            .into_any_element()
    }

    fn scene_argument_details(
        &self,
        argument: &SceneVariableRow,
        render: &SceneSettingsRenderCtx<'_>,
    ) -> gpui::AnyElement {
        let schema = match &argument.variable.source {
            zerium_core::timeline::SceneVariableSource::Argument { schema, .. } => schema,
            zerium_core::timeline::SceneVariableSource::Calculation { program, .. } => {
                return self
                    .calculation_details(argument, program, render)
                    .into_any_element();
            }
        };
        let mut details = div().w_full().flex().flex_col().gap_2();
        if let Some(settings) = self.numeric_scene_argument_settings(argument, render) {
            details = details.child(settings);
            if !render.is_main_scene
                && let Some(Ok(applied)) = &argument.result
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
        if let Some(input) = self.store.inputs.text(&ControlId::scene_argument_default(
            argument.scene_id,
            argument.variable.id(),
        )) {
            details = details.child(scene_value_row(
                render.argument_value_label(),
                Input::new(&input).small().w_full(),
            ));
        }
        if let Some(picker) = self.store.inputs.color(&ControlId::scene_argument_color(
            argument.scene_id,
            argument.variable.id(),
        )) {
            details = details.child(scene_value_row(
                render.argument_value_label(),
                crate::ui::panels::preview::color_input(&picker),
            ));
        }
        if let PropertyValue::Bool(checked) = schema.default_value() {
            details = details.child(scene_value_row(
                render.argument_value_label(),
                scene_bool_input(argument.variable.id().to_owned(), checked, render.editor),
            ));
        }
        if let PropertyValue::File(file) = schema.default_value() {
            details = details.child(scene_value_row(
                render.argument_value_label(),
                crate::ui::components::input::file::file_picker(
                    &render.file_input,
                    SharedString::from(format!("argument-file-{}", argument.variable.id())),
                    FileTarget::Argument {
                        scene_id: argument.scene_id,
                        argument_id: argument.variable.id().to_owned(),
                    },
                    crate::ui::components::input::file::FilePickerOptions {
                        path: file.as_deref(),
                        directory: schema.configuration_ui(None).selects_directory(),
                        selecting: render.selecting_file,
                        disabled: false,
                    },
                ),
            ));
        }
        details.into_any_element()
    }

    fn numeric_scene_argument_settings(
        &self,
        argument: &SceneVariableRow,
        render: &SceneSettingsRenderCtx<'_>,
    ) -> Option<gpui::AnyElement> {
        let rows = NumericSetting::visible(render.is_main_scene)
            .iter()
            .copied()
            .map(|setting| {
                let label = match setting {
                    NumericSetting::Default => render.argument_value_label(),
                    NumericSetting::Min => t!("args.minimum").to_string(),
                    NumericSetting::Max => t!("args.maximum").to_string(),
                };
                let input = self.store.inputs.text(&ControlId::scene_argument_setting(
                    argument.scene_id,
                    argument.variable.id(),
                    setting,
                ))?;
                Some(scene_argument_setting_row(
                    label,
                    &input,
                    argument.scene_id,
                    argument.variable.id(),
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

fn scene_variable_add_button(
    editor: &Entity<TimelineEditor>,
    calculation: bool,
) -> impl IntoElement {
    let create_editor = editor.clone();
    Button::new(if calculation {
        "create-scene-calculation"
    } else {
        "create-scene-argument"
    })
    .small()
    .compact()
    .icon(IconName::Plus)
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
                        editor.create_scene_calculation(ty, |number| {
                            t!("calc.new", number = number).to_string()
                        })
                    } else {
                        editor.create_scene_argument(ty, |number| {
                            t!("args.new_argument", number = number).to_string()
                        })
                    };
                    if created.is_some() {
                        cx.notify();
                    }
                });
            }))
        })
    })
}

fn scene_argument_header(
    index: usize,
    count: usize,
    argument: &SceneVariableRow,
    name_input: Option<Entity<InputState>>,
    render: &SceneSettingsRenderCtx<'_>,
) -> Div {
    let move_up_editor = render.editor.clone();
    let move_up_id = argument.variable.id().to_owned();
    let move_down_editor = render.editor.clone();
    let move_down_id = argument.variable.id().to_owned();
    let remove_settings = render.settings.clone();
    let remove_id = argument.variable.id().to_owned();

    div()
        .w_full()
        .min_w_0()
        .flex()
        .flex_none()
        .items_center()
        .gap_2()
        .when(argument.variable.calculation().is_some(), |this| {
            let label = match argument.variable.ty() {
                ScalarPropertyType::F32 => t!("args.number"),
                ScalarPropertyType::I32 => t!("args.integer"),
                ScalarPropertyType::U32 => t!("args.unsigned_integer"),
                ScalarPropertyType::Bool => t!("args.boolean"),
                ScalarPropertyType::String => t!("args.string"),
                _ => t!("args.color"),
            };
            this.child(
                div()
                    .flex_none()
                    .whitespace_nowrap()
                    .text_xs()
                    .text_color(render.colors.muted_foreground)
                    .child(label.to_string()),
            )
        })
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
                .child(
                    t!(
                        "args.binding_count",
                        count = argument.variable.bindings.len()
                    )
                    .to_string(),
                ),
        )
        .child(
            Button::new(SharedString::from(format!(
                "move-scene-argument-up-{}",
                argument.variable.id()
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
                argument.variable.id()
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
                argument.variable.id()
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
    scene_value_row(
        label,
        crate::ui::components::input::number::number_input_drag(
            settings,
            input,
            NumberInput::new(input).small().w_full(),
            false,
            move |this, event, cx| {
                this.prepare_scene_argument_value_drag(scene_id, &argument_id, setting, event, cx)
            },
        ),
    )
    .into_any_element()
}

fn scene_value_row(label: String, input: impl IntoElement) -> Div {
    div()
        .w_full()
        .min_w_0()
        .flex()
        .items_center()
        .gap_3()
        .child(property_label_column(label))
        .child(div().w_0().min_w_0().flex_1().flex().child(input))
}

fn scene_bool_input(argument_id: String, checked: bool, editor: &Entity<TimelineEditor>) -> Switch {
    let editor = editor.clone();
    Switch::new(SharedString::from(format!(
        "scene-argument-default-{argument_id}"
    )))
    .small()
    .checked(checked)
    .on_click(move |checked, _, cx| {
        editor.update(cx, |editor, cx| {
            if editor.update_scene_argument_default(&argument_id, PropertyValue::Bool(*checked))
                == Ok(true)
            {
                cx.notify();
            }
        });
    })
}
