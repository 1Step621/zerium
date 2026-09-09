use super::*;

impl PropertyInspector {
    pub(super) fn ensure_array_inputs(
        &mut self,
        item: &TimelineItem,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let numeric_fields = Self::array_property_fields(item).into_iter().chain(
            item.effects
                .iter()
                .flat_map(Self::effect_array_property_fields),
        );
        self.ensure_numeric_inputs(item, numeric_fields, window, cx);

        let array_fields = Self::array_fields(item)
            .into_iter()
            .chain(item.effects.iter().flat_map(Self::effect_array_fields));
        let array_fields: Vec<_> = array_fields.collect();
        for field in &array_fields {
            if field.element_editor == ArrayElementEditor::FontFamily {
                continue;
            }
            for (index, value) in field.values.iter().enumerate() {
                let controls = Self::array_controls(field, index, value);
                self.ensure_string_inputs(item, Self::string_fields(controls), window, cx);
            }
        }
    }

    fn update_array_parameter(
        editor: &mut TimelineEditor,
        effect_id: Option<EffectInstanceId>,
        parameter_id: &str,
        value: ParameterValue,
    ) -> bool {
        match effect_id {
            Some(effect_id) => {
                editor.update_selected_effect_parameter(effect_id, parameter_id, value)
            }
            None => editor.update_selected_parameter(parameter_id, value),
        }
    }

    fn set_array_element(
        editor: &mut TimelineEditor,
        effect_id: Option<EffectInstanceId>,
        parameter_id: &str,
        index: usize,
        value: ParameterValue,
    ) -> bool {
        let Some(item) = editor.selected_item() else {
            return false;
        };
        let current = match effect_id {
            Some(effect_id) => item
                .effects
                .iter()
                .find(|effect| effect.id == effect_id)
                .and_then(|effect| effect.parameters.get(parameter_id)),
            None => item.parameters.get(parameter_id),
        };
        let Some(ParameterValue::Array(current)) = current else {
            return false;
        };
        let mut updated = current.clone();
        let Some(element) = updated.get_mut(index) else {
            return false;
        };
        *element = value;
        Self::update_array_parameter(
            editor,
            effect_id,
            parameter_id,
            ParameterValue::Array(updated),
        )
    }

    fn push_array_element(
        editor: &mut TimelineEditor,
        effect_id: Option<EffectInstanceId>,
        parameter_id: &str,
        value: ParameterValue,
    ) -> bool {
        let Some(item) = editor.selected_item() else {
            return false;
        };
        let current = match effect_id {
            Some(effect_id) => item
                .effects
                .iter()
                .find(|effect| effect.id == effect_id)
                .and_then(|effect| effect.parameters.get(parameter_id)),
            None => item.parameters.get(parameter_id),
        };
        let Some(ParameterValue::Array(current)) = current else {
            return false;
        };
        let mut updated = current.clone();
        updated.push(value);
        Self::update_array_parameter(
            editor,
            effect_id,
            parameter_id,
            ParameterValue::Array(updated),
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn array_field_element(
        item: &TimelineItem,
        field: ArrayField,
        inputs: &HashMap<PropertyPath, Entity<InputState>>,
        animation_inputs: &HashMap<PropertyPath, (Entity<InputState>, Entity<InputState>)>,
        color_pickers: &HashMap<PropertyPath, Entity<ColorPickerState>>,
        animation_color_pickers: &HashMap<
            (PropertyPath, AnimationEndpoint),
            Entity<ColorPickerState>,
        >,
        editor: &Entity<TimelineEditor>,
        inspector: &Entity<Self>,
        focus_handle: &FocusHandle,
        separator_color: gpui::Hsla,
        allow_structure_edit: bool,
        editing_scene: bool,
        scene_arguments: &[SceneArgumentOption],
        font_names: &[String],
    ) -> gpui::AnyElement {
        let ParameterType::Array {
            min_items,
            max_items,
            ..
        } = field.parameter.ty()
        else {
            unreachable!("array field schema");
        };
        let item_id = item.id;
        let owner = SceneBindingOwner::from_effect(field.target.effect_id);
        let parameter_label = field.parameter.label().to_owned();
        let elements_are_tuples = matches!(
            field.parameter.ty().element_type(),
            ParameterValueType::Tuple(_)
        );
        let array_has_scene_binding = Self::array_has_scene_binding(
            item_id,
            owner,
            &field.target.parameter_id,
            scene_arguments,
        );
        let mut rows = div().w_full().min_w_0().flex().flex_col().gap_1();
        let tuple_ctx = tuple::TupleRowContext {
            item_id,
            editing_scene,
            scene_arguments,
            item,
            inputs,
            animation_inputs,
            color_pickers,
            animation_color_pickers,
            editor,
            inspector,
            focus_handle,
            size_locked: false,
            muted_color: None,
        };
        for (element, element_value) in field.values.iter().enumerate() {
            let element_animation_enabled = item
                .animation(
                    field.target.effect_id,
                    &field.target.parameter_id,
                    Some(element),
                )
                .is_some();
            let scene_binding = (field.element_editor == ArrayElementEditor::FontFamily)
                .then(|| {
                    Self::scene_field_binding(
                        editing_scene,
                        element_animation_enabled,
                        field.parameter.is_scene_bindable(),
                        SceneBindingTarget::new(
                            item_id,
                            owner,
                            field.target.parameter_id.clone(),
                            SceneBindingValuePath::ArrayElement(element),
                        ),
                        &ParameterType::Value(field.parameter.ty().element_type().clone()),
                        scene_arguments,
                    )
                })
                .flatten();
            let is_scene_bound = scene_binding
                .as_ref()
                .is_some_and(|binding| binding.connected.is_some());
            let binding_button = scene_binding.map(|binding| {
                Self::scene_binding_button(
                    binding,
                    inspector,
                    SharedString::from(format!(
                        "bind-scene-array-{}-{element}",
                        field.target.parameter_id
                    )),
                )
            });
            let mut element_has_binding = is_scene_bound;
            let mut value_rows = div()
                .min_w_0()
                .flex()
                .flex_col()
                .gap_1()
                .when(elements_are_tuples, |this| this.w_full())
                .when(!elements_are_tuples, |this| this.flex_1());
            if field.element_editor != ArrayElementEditor::FontFamily {
                for control in Self::array_controls(&field, element, element_value) {
                    match control {
                        PropertyControl::Number(property) => {
                            if property.target.value_path.tuple_element().is_some() {
                                let Some((row, bound)) =
                                    Self::tuple_number_row(property, &tuple_ctx)
                                else {
                                    continue;
                                };
                                element_has_binding |= bound;
                                value_rows = value_rows.child(row);
                                continue;
                            }
                            let component = property.target.value_path.tuple_element();
                            let Some(input) = inputs.get(&property.target.key) else {
                                continue;
                            };
                            let animation_enabled =
                                Self::number_animation(item, &property).is_some();
                            let scene_binding = Self::scene_binding_for_property(
                                editing_scene,
                                animation_enabled,
                                item_id,
                                &property,
                                scene_arguments,
                            );
                            let component_is_bound = scene_binding
                                .as_ref()
                                .is_some_and(|binding| binding.connected.is_some());
                            element_has_binding |= component_is_bound;
                            let component_binding_button = scene_binding.map(|binding| {
                                Self::scene_binding_button(
                                    binding,
                                    inspector,
                                    SharedString::from(format!(
                                        "bind-scene-array-{}-{element}-{component:?}",
                                        field.target.parameter_id
                                    )),
                                )
                            });
                            let value_input = if let Some((from, to)) = animation_inputs
                                .get(&property.target.key)
                                .filter(|_| animation_enabled)
                            {
                                div()
                                    .min_w_0()
                                    .flex()
                                    .flex_1()
                                    .gap_1()
                                    .child(Self::animated_number_input(
                                        &property,
                                        AnimationEndpoint::From,
                                        from,
                                        inspector,
                                        focus_handle,
                                        false,
                                        false,
                                    ))
                                    .child(Self::animated_number_input(
                                        &property,
                                        AnimationEndpoint::To,
                                        to,
                                        inspector,
                                        focus_handle,
                                        true,
                                        false,
                                    ))
                                    .into_any_element()
                            } else {
                                NumberInput::new(input)
                                    .small()
                                    .w_full()
                                    .suffix(div().text_sm().child(property.input.suffix.clone()))
                                    .into_any_element()
                            };
                            let animation_button = (property.animatable && !component_is_bound)
                                .then(|| {
                                    let animation_inspector = inspector.clone();
                                    let animation_property = property.clone();
                                    Button::new(SharedString::from(format!(
                                        "toggle-animation-{}",
                                        property.target.key
                                    )))
                                    .icon(Icon::new(IconName::Keyframe))
                                    .small()
                                    .compact()
                                    .ghost()
                                    .selected(animation_enabled)
                                    .tooltip(if animation_enabled {
                                        "この座標のアニメーションを解除"
                                    } else {
                                        "要素全体をアニメーションする"
                                    })
                                    .on_click(
                                        move |_, window, cx| {
                                            animation_inspector.update(cx, |inspector, cx| {
                                                inspector.set_number_animation_enabled(
                                                    &animation_property,
                                                    !animation_enabled,
                                                    window,
                                                    cx,
                                                );
                                            });
                                        },
                                    )
                                });
                            let drag = PropertyValueDrag {
                                inspector_id: inspector.entity_id(),
                                path: property.target.key.clone(),
                                animation_endpoint: None,
                            };
                            let select_inspector = inspector.clone();
                            let select_property = property.clone();
                            let drag_inspector = inspector.clone();
                            let drag_property = property.clone();
                            let drag_input = input.clone();
                            let drag_focus_handle = focus_handle.clone();
                            value_rows = value_rows.child(
                                div()
                                    .min_w_0()
                                    .w_full()
                                    .flex()
                                    .items_center()
                                    .gap_2()
                                    .when(!component_is_bound, |this| {
                                        this.child(
                                            div()
                                                .id(SharedString::from(
                                                    property.target.key.to_string(),
                                                ))
                                                .min_w_0()
                                                .flex()
                                                .flex_1()
                                                .on_mouse_down(
                                                    MouseButton::Left,
                                                    move |event, _, cx| {
                                                        select_inspector.update(
                                                            cx,
                                                            |inspector, cx| {
                                                                inspector.select_number_animation(
                                                                    &select_property,
                                                                    cx,
                                                                );
                                                            },
                                                        );
                                                        if !animation_enabled {
                                                            drag_inspector.update(
                                                                cx,
                                                                |inspector, cx| {
                                                                    inspector.prepare_value_drag(
                                                                        &drag_property,
                                                                        event,
                                                                        None,
                                                                        cx,
                                                                    );
                                                                },
                                                            );
                                                        }
                                                    },
                                                )
                                                .when(!animation_enabled, move |this| {
                                                    this.on_drag(
                                                        drag,
                                                        move |drag, _, window, cx| {
                                                            cx.stop_propagation();
                                                            drag_input.update(cx, |input, cx| {
                                                                input.unselect(window, cx)
                                                            });
                                                            drag_focus_handle.focus(window, cx);
                                                            cx.new(|_| drag.clone())
                                                        },
                                                    )
                                                })
                                                .child(value_input),
                                        )
                                    })
                                    .when_some(animation_button, |this, button| this.child(button))
                                    .when_some(component_binding_button, |this, button| {
                                        this.child(button)
                                    }),
                            );
                        }

                        PropertyControl::Color(field) => {
                            let animation_enabled = field.target.animation_enabled(item);
                            let binding = Self::scene_field_binding(
                                editing_scene,
                                animation_enabled,
                                field.scene_bindable,
                                SceneBindingTarget::new(
                                    item_id,
                                    owner,
                                    field.target.parameter_id.clone(),
                                    field.target.value_path,
                                ),
                                &ParameterType::Value(ParameterValueType::Scalar(
                                    ScalarParameterType::Color,
                                )),
                                scene_arguments,
                            );
                            element_has_binding |= binding
                                .as_ref()
                                .is_some_and(|binding| binding.connected.is_some());
                            if let Some(picker) = color_pickers.get(&field.target.key) {
                                value_rows = value_rows.child(Self::color_field_element(
                                    field,
                                    picker,
                                    animation_color_pickers,
                                    animation_enabled,
                                    binding,
                                    inspector,
                                ));
                            }
                        }
                        PropertyControl::String(field) => {
                            if let Some(row) = Self::string_field_element(
                                field,
                                item_id,
                                editing_scene,
                                scene_arguments,
                                inspector,
                                inputs,
                            ) {
                                value_rows = value_rows.child(row);
                            }
                        }
                        PropertyControl::Bool(field) => {
                            let binding = Self::scene_field_binding(
                                editing_scene,
                                false,
                                field.scene_bindable,
                                SceneBindingTarget::new(
                                    item_id,
                                    owner,
                                    field.target.parameter_id.clone(),
                                    field.target.value_path,
                                ),
                                &ParameterType::Value(ParameterValueType::Scalar(
                                    ScalarParameterType::Bool,
                                )),
                                scene_arguments,
                            );
                            value_rows = value_rows
                                .child(Self::bool_field_element(field, editor, binding, inspector));
                        }
                        PropertyControl::Choice(field) => {
                            let binding = Self::scene_field_binding(
                                editing_scene,
                                false,
                                field.scene_bindable,
                                SceneBindingTarget::new(
                                    item_id,
                                    owner,
                                    field.target.parameter_id.clone(),
                                    field.target.value_path,
                                ),
                                &field.ty,
                                scene_arguments,
                            );
                            value_rows = value_rows.child(Self::choice_field_element(
                                field, editor, binding, inspector,
                            ))
                        }
                        PropertyControl::Tuple(tuple) => {
                            for control in tuple.controls {
                                for (row, bound) in Self::tuple_control_rows(control, &tuple_ctx) {
                                    element_has_binding |= bound;
                                    value_rows = value_rows.child(row);
                                }
                            }
                        }
                        PropertyControl::Array(_) => unreachable!("array elements are values"),
                    }
                }
            }

            match field.element_editor {
                ArrayElementEditor::Scalar => {}
                ArrayElementEditor::FontFamily => {
                    let ParameterValue::String(selected_font) = element_value else {
                        continue;
                    };
                    let font_choices = font_names
                        .iter()
                        .filter(|font| {
                            !field.values.iter().enumerate().any(|(index, value)| {
                                index != element
                                    && matches!(value, ParameterValue::String(selected) if selected == *font)
                            })
                        })
                        .map(|font| SearchPickerEntry::new(font.clone(), "", font.clone()))
                        .collect::<Vec<_>>();
                    let picker_editor = editor.clone();
                    let picker_parameter_id = field.target.parameter_id.clone();
                    let picker_effect_id = field.target.effect_id;
                    let label = if selected_font.is_empty() {
                        "フォントを選択".to_owned()
                    } else {
                        selected_font.clone()
                    };
                    let mut trigger = Button::new(SharedString::from(format!(
                        "{}-array-{element}-font",
                        field.target.key
                    )))
                    .small()
                    .w_full()
                    .label(label)
                    .dropdown_caret(true);
                    let trigger_style = trigger.style().clone();
                    value_rows = value_rows.child(
                        Popover::new(SharedString::from(format!(
                            "{}-array-{element}-font-picker",
                            field.target.key
                        )))
                        .trigger_style(trigger_style)
                        .trigger(trigger)
                        .content(move |window, cx| {
                            let editor = picker_editor.clone();
                            let parameter_id = picker_parameter_id.clone();
                            let entries = font_choices.clone();
                            cx.new(|cx| {
                                SearchPicker::new(
                                    entries,
                                    "フォントを検索",
                                    move |font, _, cx| {
                                        editor.update(cx, |editor, cx| {
                                            if editor
                                                .selected_item()
                                                .is_some_and(|item| item.id == item_id)
                                                && Self::set_array_element(
                                                    editor,
                                                    picker_effect_id,
                                                    &parameter_id,
                                                    element,
                                                    ParameterValue::String(font),
                                                )
                                            {
                                                cx.notify();
                                            }
                                        });
                                    },
                                    window,
                                    cx,
                                )
                            })
                        }),
                    );
                }
            }

            let move_up_editor = editor.clone();
            let move_up_parameter_id = field.target.parameter_id.clone();
            let move_up_effect_id = field.target.effect_id;
            let mut moved_up = field.values.clone();
            if element > 0 {
                moved_up.swap(element, element - 1);
            }
            let move_up_button = Button::new(SharedString::from(format!(
                "array-{}-{}-{element}-up",
                item_id.get(),
                field.target.parameter_id
            )))
            .small()
            .compact()
            .ghost()
            .icon(IconName::ChevronUp)
            .tooltip("上へ移動")
            .disabled(element == 0 || array_has_scene_binding)
            .on_click(move |_, _, cx| {
                move_up_editor.update(cx, |editor, cx| {
                    if editor
                        .selected_item()
                        .is_some_and(|item| item.id == item_id)
                        && Self::update_array_parameter(
                            editor,
                            move_up_effect_id,
                            &move_up_parameter_id,
                            ParameterValue::Array(moved_up.clone()),
                        )
                    {
                        cx.notify();
                    }
                });
            });

            let move_down_editor = editor.clone();
            let move_down_parameter_id = field.target.parameter_id.clone();
            let move_down_effect_id = field.target.effect_id;
            let mut moved_down = field.values.clone();
            if element + 1 < moved_down.len() {
                moved_down.swap(element, element + 1);
            }
            let move_down_button = Button::new(SharedString::from(format!(
                "array-{}-{}-{element}-down",
                item_id.get(),
                field.target.parameter_id
            )))
            .small()
            .compact()
            .ghost()
            .icon(IconName::ChevronDown)
            .tooltip("下へ移動")
            .disabled(element + 1 == field.values.len() || array_has_scene_binding)
            .on_click(move |_, _, cx| {
                move_down_editor.update(cx, |editor, cx| {
                    if editor
                        .selected_item()
                        .is_some_and(|item| item.id == item_id)
                        && Self::update_array_parameter(
                            editor,
                            move_down_effect_id,
                            &move_down_parameter_id,
                            ParameterValue::Array(moved_down.clone()),
                        )
                    {
                        cx.notify();
                    }
                });
            });

            let remove_editor = editor.clone();
            let remove_parameter_id = field.target.parameter_id.clone();
            let remove_effect_id = field.target.effect_id;
            let mut remaining = field.values.clone();
            remaining.remove(element);
            let remove_button = Button::new(SharedString::from(format!(
                "array-{}-{}-{element}-remove",
                item_id.get(),
                field.target.parameter_id
            )))
            .small()
            .compact()
            .ghost()
            .icon(IconName::Delete)
            .tooltip("削除")
            .disabled(
                field.values.len() <= *min_items as usize
                    || element_has_binding
                    || array_has_scene_binding,
            )
            .on_click(move |_, _, cx| {
                remove_editor.update(cx, |editor, cx| {
                    if editor
                        .selected_item()
                        .is_some_and(|item| item.id == item_id)
                        && Self::update_array_parameter(
                            editor,
                            remove_effect_id,
                            &remove_parameter_id,
                            ParameterValue::Array(remaining.clone()),
                        )
                    {
                        cx.notify();
                    }
                });
            });
            let structure_buttons = div()
                .flex_none()
                .flex()
                .items_center()
                .gap_1()
                .child(move_up_button)
                .child(move_down_button)
                .child(remove_button);
            let row = if elements_are_tuples {
                div()
                    .w_full()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .pb_2()
                    .when(element > 0, |this| {
                        this.pt_2().border_t_1().border_color(separator_color)
                    })
                    .child(
                        div()
                            .w_full()
                            .h(px(24.))
                            .flex()
                            .items_center()
                            .child(
                                div()
                                    .flex_1()
                                    .text_sm()
                                    .child(format!("要素 {}", element + 1)),
                            )
                            .when(allow_structure_edit, |this| this.child(structure_buttons))
                            .when_some(binding_button, |this, button| this.child(button)),
                    )
                    .when(!is_scene_bound, |this| this.child(value_rows))
            } else {
                div()
                    .w_full()
                    .min_w_0()
                    .flex()
                    .items_center()
                    .gap_1()
                    .when(!is_scene_bound, |this| this.child(value_rows))
                    .when_some(binding_button, |this, button| this.child(button))
                    .when(allow_structure_edit, |this| this.child(structure_buttons))
            };
            rows = rows.child(row);
        }

        let add_disabled = field.values.len() >= *max_items as usize || array_has_scene_binding;
        let add_editor = editor.clone();
        let add_parameter_id = field.target.parameter_id.clone();
        let add_effect_id = field.target.effect_id;
        let next_value = model::append_default(&field);
        let add_control = Button::new(SharedString::from(format!(
            "array-{}-{}-add",
            item_id.get(),
            field.target.parameter_id
        )))
        .small()
        .w_full()
        .label(format!("{}を追加", field.parameter.label()))
        .disabled(add_disabled)
        .on_click(move |_, _, cx| {
            add_editor.update(cx, |editor, cx| {
                if editor
                    .selected_item()
                    .is_some_and(|item| item.id == item_id)
                    && Self::push_array_element(
                        editor,
                        add_effect_id,
                        &add_parameter_id,
                        next_value.clone(),
                    )
                {
                    cx.notify();
                }
            });
        });

        div()
            .w_full()
            .flex()
            .items_start()
            .gap_3()
            .child(Self::parameter_label_column(parameter_label))
            .child(
                div()
                    .w_0()
                    .min_w_0()
                    .flex_1()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(rows)
                    .when(allow_structure_edit, |this| {
                        this.child(div().pt_1().child(add_control))
                    }),
            )
            .into_any_element()
    }
}
