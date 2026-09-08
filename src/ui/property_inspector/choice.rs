use super::*;

impl PropertyInspector {
    pub(super) fn choice_field_element(
        field: ChoiceField,
        editor: &Entity<TimelineEditor>,
        binding: Option<SceneFieldBinding>,
        inspector: &Entity<Self>,
    ) -> gpui::AnyElement {
        let bound = binding
            .as_ref()
            .is_some_and(|binding| binding.connected.is_some());
        let binding_button = binding.map(|binding| {
            Self::scene_binding_button(
                binding,
                inspector,
                SharedString::from(format!("bind-{}", field.target.key)),
            )
        });
        let value_path = field.target.value_path;
        let parameter_id = field.target.parameter_id;
        let effect_id = field.target.effect_id;
        let selected_label = field
            .options
            .iter()
            .find(|(_, value)| *value == field.value)
            .map(|(label, _)| label.clone())
            .unwrap_or_default();
        let options = field.options;
        let editor = editor.clone();
        div()
            .w_full()
            .flex()
            .items_center()
            .gap_3()
            .child(Self::parameter_label_column(field.label))
            .when(!bound, |row| {
                row.child(
                    Button::new(SharedString::from(field.target.key.to_string()))
                        .small()
                        .w_full()
                        .label(selected_label)
                        .dropdown_caret(true)
                        .popup_menu(move |menu, _, _| {
                            options.iter().fold(menu, |menu, (label, value)| {
                                let editor = editor.clone();
                                let parameter_id = parameter_id.clone();
                                let value = *value;
                                menu.item(PopupMenuItem::new(label.clone()).on_click(
                                    move |_, _, cx| {
                                        editor.update(cx, |editor, cx| {
                                            let changed = Self::set_scalar_value(
                                                editor,
                                                effect_id,
                                                &parameter_id,
                                                value_path,
                                                ParameterValue::Enum(value),
                                            );
                                            if changed {
                                                cx.notify();
                                            }
                                        });
                                    },
                                ))
                            })
                        }),
                )
            })
            .when_some(binding_button, |row, button| row.child(button))
            .into_any_element()
    }

    pub(super) fn bool_field_element(
        field: BoolField,
        editor: &Entity<TimelineEditor>,
        scene_binding: Option<SceneFieldBinding>,
        inspector: &Entity<Self>,
    ) -> gpui::AnyElement {
        let editor = editor.clone();
        let is_scene_bound = scene_binding
            .as_ref()
            .is_some_and(|binding| binding.connected.is_some());
        let binding_button = scene_binding.map(|binding| {
            Self::scene_binding_button(
                binding,
                inspector,
                SharedString::from(format!("bind-scene-argument-{}", field.target.key)),
            )
        });
        let value_path = field.target.value_path;
        let parameter_id = field.target.parameter_id;
        let effect_id = field.target.effect_id;
        let checked = field.value && !field.mixed;
        let mixed = field.mixed;
        div()
            .w_full()
            .flex()
            .items_center()
            .gap_3()
            .child(Self::parameter_label_column(field.label))
            .child(
                div()
                    .min_w_0()
                    .flex_1()
                    .flex()
                    .items_center()
                    .gap_2()
                    .when(!is_scene_bound && mixed, |this| {
                        this.child(div().text_xs().child("混在"))
                    })
                    .when(!is_scene_bound, |this| {
                        this.child(
                            Switch::new(SharedString::from(field.target.key.to_string()))
                                .small()
                                .checked(checked)
                                .tooltip(if mixed {
                                    "値が混在しています。クリックですべてオン"
                                } else if checked {
                                    "オフにする"
                                } else {
                                    "オンにする"
                                })
                                .on_click(move |checked, _, cx| {
                                    editor.update(cx, |editor, cx| {
                                        let changed = Self::set_scalar_value(
                                            editor,
                                            effect_id,
                                            &parameter_id,
                                            value_path,
                                            ParameterValue::Bool(*checked),
                                        );
                                        if changed {
                                            cx.notify();
                                        }
                                    });
                                }),
                        )
                    })
                    .when_some(binding_button, |this, button| this.child(button)),
            )
            .into_any_element()
    }
}
