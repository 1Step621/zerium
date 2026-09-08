use super::*;

impl PropertyInspector {
    pub(super) fn scene_settings_element(
        &self,
        arguments: &[SceneArgumentOption],
        render: &InspectorRenderContext<'_>,
    ) -> gpui::AnyElement {
        let active_scene_name = render.active_scene_name_input.clone();
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
        if let Some(input) = active_scene_name {
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
        render: &InspectorRenderContext<'_>,
    ) -> gpui::AnyElement {
        let expanded = self
            .expanded_scene_arguments
            .contains(&(argument.scene_id, argument.id.clone()));
        let name_input = self
            .controls
            .scene_argument_name_inputs
            .get(&(argument.scene_id, argument.id.clone()))
            .cloned();
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
        render: &InspectorRenderContext<'_>,
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
        render: &InspectorRenderContext<'_>,
    ) -> gpui::AnyElement {
        if argument.derived {
            return self
                .controls
                .scene_argument_expression_inputs
                .get(&(argument.scene_id, argument.id.clone()))
                .cloned()
                .map(|input| Self::scene_text_input_row("式", input).into_any_element())
                .unwrap_or_else(|| div().into_any_element());
        }

        let mut details = div().w_full().flex().flex_col().gap_2();
        if let Some(settings) = self.numeric_scene_argument_settings(argument, render) {
            details = details.child(settings);
        }
        if let Some(input) = self
            .controls
            .scene_argument_default_inputs
            .get(&(argument.scene_id, argument.id.clone()))
            .cloned()
        {
            details = details.child(Self::scene_text_input_row("デフォルト", input));
        }
        if let Some(picker) = self
            .controls
            .scene_argument_color_pickers
            .get(&(argument.scene_id, argument.id.clone()))
            .cloned()
        {
            details = details.child(Self::scene_color_row(picker));
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
        render: &InspectorRenderContext<'_>,
    ) -> Option<gpui::AnyElement> {
        let input = |setting| {
            self.controls
                .scene_argument_setting_inputs
                .get(&(argument.scene_id, argument.id.clone(), setting))
                .cloned()
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
