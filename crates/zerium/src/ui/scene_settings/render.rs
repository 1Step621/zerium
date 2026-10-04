use super::*;

#[derive(Clone)]
struct SceneArgumentOption {
    scene_id: SceneId,
    id: String,
    schema: PropertySchema,
    binding_count: usize,
}

struct SceneSettingsRenderCtx<'a> {
    colors: ThemeColor,
    editor: &'a Entity<TimelineEditor>,
    settings: Entity<SceneSettings>,
    active_scene_name_input: Option<Entity<InputState>>,
}

impl Render for SceneSettings {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let args = self
            .editor
            .read(cx)
            .active_scene_id()
            .and_then(|id| {
                self.editor.read(cx).scene(id).map(|scene| {
                    scene
                        .arguments
                        .iter()
                        .map(|arg| SceneArgumentOption {
                            scene_id: id,
                            id: arg.schema.id().to_owned(),
                            schema: arg.schema.clone(),
                            binding_count: arg.bindings.len(),
                        })
                        .collect::<Vec<_>>()
                })
            })
            .unwrap_or_default();
        let render = SceneSettingsRenderCtx {
            colors: cx.theme().colors,
            editor: &self.editor,
            settings: cx.entity(),
            active_scene_name_input: self.scene_id.and_then(|id| {
                self.store
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

    fn toggle_scene_argument_expanded(
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

    fn scene_settings_elements(
        &self,
        arguments: &[SceneArgumentOption],
        render: &SceneSettingsRenderCtx<'_>,
    ) -> Vec<gpui::AnyElement> {
        let active_scene_name_input = render.active_scene_name_input.clone();
        let rows = arguments
            .iter()
            .cloned()
            .enumerate()
            .map(|(index, argument)| {
                self.scene_argument_element(index, arguments.len(), argument, render)
            })
            .collect::<Vec<_>>();

        let mut elements = Vec::new();
        if let Some(input) = active_scene_name_input {
            elements.push(
                Self::scene_text_input_row(t!("args.scene_name").to_string(), input)
                    .flex_none()
                    .into_any_element(),
            );
        }
        elements.push(
            Self::scene_arguments_header(render.editor)
                .flex_none()
                .into_any_element(),
        );
        elements.extend(rows);
        elements
    }

    fn scene_arguments_header(editor: &Entity<TimelineEditor>) -> Div {
        let create_editor = editor.clone();
        div()
            .w_full()
            .flex()
            .items_center()
            .justify_between()
            .child(
                div()
                    .text_sm()
                    .child(t!("args.scene_arguments").to_string()),
            )
            .child(
                Button::new("create-scene-argument")
                    .small()
                    .compact()
                    .label(t!("args.create").to_string())
                    .dropdown_caret(true)
                    .popup_menu(move |menu, _, _| {
                        [
                            (t!("args.number").to_string(), SceneArgumentPreset::Number),
                            (
                                t!("args.integer").to_string(),
                                SceneArgumentPreset::SignedInteger,
                            ),
                            (
                                t!("args.unsigned_integer").to_string(),
                                SceneArgumentPreset::UnsignedInteger,
                            ),
                            (t!("args.boolean").to_string(), SceneArgumentPreset::Boolean),
                            (t!("args.color").to_string(), SceneArgumentPreset::Color),
                            (t!("args.string").to_string(), SceneArgumentPreset::Text),
                        ]
                        .into_iter()
                        .fold(menu, |menu, (label, ty)| {
                            let editor = create_editor.clone();
                            menu.item(PopupMenuItem::new(label).on_click(move |_, _, cx| {
                                editor.update(cx, |editor, cx| {
                                    if editor
                                        .create_scene_argument(ty, |ordinal| {
                                            t!("args.new_argument", number = ordinal).to_string()
                                        })
                                        .is_some()
                                    {
                                        cx.notify();
                                    }
                                });
                            }))
                        })
                    }),
            )
    }

    fn scene_argument_element(
        &self,
        index: usize,
        count: usize,
        argument: SceneArgumentOption,
        render: &SceneSettingsRenderCtx<'_>,
    ) -> gpui::AnyElement {
        let expanded = self
            .expanded_scene_arguments
            .contains(&(argument.scene_id, argument.id.clone()));
        let name_input = self
            .store
            .text_inputs
            .get(&ControlId::scene_argument_name(
                argument.scene_id,
                &argument.id,
            ))
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
        render: &SceneSettingsRenderCtx<'_>,
    ) -> Div {
        let move_up_editor = render.editor.clone();
        let move_up_id = argument.id.clone();
        let move_down_editor = render.editor.clone();
        let move_down_id = argument.id.clone();
        let expand_settings = render.settings.clone();
        let expand_id = argument.id.clone();
        let remove_settings = render.settings.clone();
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
                .tooltip(t!("common.move_down").to_string())
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
                    t!("args.collapse").to_string()
                } else {
                    t!("args.expand").to_string()
                })
                .on_click(move |_, _, cx| {
                    expand_settings.update(cx, |settings, cx| {
                        settings.toggle_scene_argument_expanded(scene_id, &expand_id, cx);
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
                            let result = editor.remove_scene_argument(&remove_id);
                            if result.is_ok() {
                                cx.notify();
                            }
                            result
                        });
                        if result.is_err() {
                            settings.notifications.update(cx, |notifications, cx| {
                                notifications.push(t!("args.remove_failed").to_string(), cx);
                            });
                        }
                    });
                }),
            )
    }

    fn scene_argument_details(
        &self,
        argument: &SceneArgumentOption,
        render: &SceneSettingsRenderCtx<'_>,
    ) -> gpui::AnyElement {
        let mut details = div().w_full().flex().flex_col().gap_2();
        if let Some(settings) = self.numeric_scene_argument_settings(argument, render) {
            details = details.child(settings);
        }
        if let Some(input) = self
            .store
            .text_inputs
            .get(&ControlId::scene_argument_default(
                argument.scene_id,
                &argument.id,
            ))
        {
            details = details.child(Self::scene_text_input_row(
                t!("args.default").to_string(),
                input.input.clone(),
            ));
        }
        if let Some(picker) = self
            .store
            .color_pickers
            .get(&ControlId::scene_argument_color(
                argument.scene_id,
                &argument.id,
            ))
        {
            details = details.child(Self::scene_color_row(picker.picker.clone()));
        }
        if let PropertyValue::Bool(checked) = argument.schema.default_value() {
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
        render: &SceneSettingsRenderCtx<'_>,
    ) -> Option<gpui::AnyElement> {
        let rows = [
            (NumericSetting::Default, t!("args.default")),
            (NumericSetting::Min, t!("args.minimum")),
            (NumericSetting::Max, t!("args.maximum")),
        ]
        .into_iter()
        .map(|(setting, label)| {
            let input = self.store.text(&ControlId::scene_argument_setting(
                argument.scene_id,
                &argument.id,
                setting,
            ))?;
            Some(Self::scene_argument_setting_row(
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

    fn scene_argument_setting_row(
        label: String,
        input: &Entity<InputState>,
        scene_id: SceneId,
        argument_id: &str,
        setting: NumericSetting,
        settings: &Entity<Self>,
    ) -> gpui::AnyElement {
        let argument_id = argument_id.to_owned();
        div()
            .w_full()
            .flex()
            .items_center()
            .gap_3()
            .child(Self::property_label_column(label))
            .child(crate::ui::number_input::number_input_drag(
                settings,
                input,
                NumberInput::new(input).small().w_full(),
                false,
                move |this, event, cx| {
                    this.prepare_scene_argument_value_drag(
                        scene_id,
                        &argument_id,
                        setting,
                        event,
                        cx,
                    )
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
            .child(Self::property_label_column(label))
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
            .child(Self::property_label_column(t!("args.default").to_string()))
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
            .child(Self::property_label_column(t!("args.default").to_string()))
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
                            PropertyValue::Bool(*checked),
                        ) {
                            cx.notify();
                        }
                    });
                }),
            )
    }
}
