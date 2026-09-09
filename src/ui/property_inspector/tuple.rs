use super::*;

/// Shared context for rendering one tuple element as a compact row with an
/// element label, the same way numeric tuple coordinates are shown.
/// Used both for direct tuple parameters and for tuple array elements.
pub(super) struct TupleRowContext<'a> {
    pub item_id: ItemId,
    pub editing_scene: bool,
    pub scene_arguments: &'a [SceneArgumentOption],
    pub item: &'a TimelineItem,
    pub inputs: &'a HashMap<PropertyPath, Entity<InputState>>,
    pub animation_inputs: &'a HashMap<PropertyPath, (Entity<InputState>, Entity<InputState>)>,
    pub color_pickers: &'a HashMap<PropertyPath, Entity<ColorPickerState>>,
    pub animation_color_pickers:
        &'a HashMap<(PropertyPath, AnimationEndpoint), Entity<ColorPickerState>>,
    pub editor: &'a Entity<TimelineEditor>,
    pub inspector: &'a Entity<PropertyInspector>,
    pub focus_handle: &'a FocusHandle,
    pub size_locked: bool,
    pub muted_color: Option<gpui::Hsla>,
}

impl PropertyInspector {
    fn tuple_element_label(label: Option<String>) -> Option<Div> {
        label.map(|label| div().w(px(32.)).flex_none().text_sm().child(label))
    }

    pub(super) fn tuple_number_row(
        field: NumberField,
        ctx: &TupleRowContext<'_>,
    ) -> Option<(gpui::AnyElement, bool)> {
        let input = ctx.inputs.get(&field.target.key)?.clone();
        let animation = ctx.animation_inputs.get(&field.target.key).cloned();
        let coordinate_animation_enabled = field.target.animation_enabled(ctx.item);
        let scene_binding = Self::scene_binding_for_property(
            ctx.editing_scene,
            Self::number_animation(ctx.item, &field).is_some(),
            ctx.item_id,
            &field,
            ctx.scene_arguments,
        );
        let is_bound = scene_binding
            .as_ref()
            .is_some_and(|binding| binding.connected.is_some());
        let binding_button = scene_binding.map(|binding| {
            Self::scene_binding_button(
                binding,
                ctx.inspector,
                SharedString::from(format!("bind-scene-argument-{}", field.target.key)),
            )
        });
        let disabled = ctx.size_locked && field.target.value_path.tuple_element() == Some(1);
        let animation_visible = coordinate_animation_enabled || (disabled && animation.is_some());
        let animation_button = (field.animatable && !is_bound).then(|| {
            let inspector = ctx.inspector.clone();
            let field = field.clone();
            Button::new(SharedString::from(format!(
                "toggle-animation-{}",
                field.target.key
            )))
            .icon(Icon::new(IconName::Keyframe))
            .small()
            .compact()
            .ghost()
            .tab_stop(!disabled)
            .selected(animation_visible)
            .tooltip(if disabled {
                "アスペクト比維持中は幅から自動計算"
            } else if coordinate_animation_enabled {
                "この座標のアニメーションを解除"
            } else {
                "この座標をアニメーションする"
            })
            .when(!disabled, |button| {
                button.on_click(move |_, window, cx| {
                    cx.stop_propagation();
                    inspector.update(cx, |inspector, cx| {
                        inspector.set_number_animation_enabled(
                            &field,
                            !coordinate_animation_enabled,
                            window,
                            cx,
                        );
                    });
                })
            })
        });
        let value_input = if let Some((from, to)) = animation {
            div()
                .w_full()
                .min_w_0()
                .flex()
                .flex_1()
                .gap_1()
                .child(Self::animated_number_input(
                    &field,
                    AnimationEndpoint::From,
                    &from,
                    ctx.inspector,
                    ctx.focus_handle,
                    false,
                    disabled,
                ))
                .child(Self::animated_number_input(
                    &field,
                    AnimationEndpoint::To,
                    &to,
                    ctx.inspector,
                    ctx.focus_handle,
                    true,
                    disabled,
                ))
                .into_any_element()
        } else {
            Self::draggable_number_input(&field, &input, ctx.inspector, ctx.focus_handle, disabled)
        };
        let select_inspector = ctx.inspector.clone();
        let select_field = field.clone();
        let row = div()
            .min_w_0()
            .w_full()
            .flex()
            .items_center()
            .gap_2()
            .when_some(ctx.muted_color.filter(|_| disabled), |this, muted| {
                this.text_color(muted)
            })
            .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                select_inspector.update(cx, |inspector, cx| {
                    inspector.select_number_animation(&select_field, cx);
                });
            })
            .when_some(
                Self::tuple_element_label(field.element_label.clone()),
                |this, label| this.child(label),
            )
            .when(!is_bound, |this| {
                this.child(div().min_w_0().flex().flex_1().child(value_input))
            })
            .when_some(animation_button, |this, button| this.child(button))
            .when_some(binding_button, |this, button| this.child(button))
            .into_any_element();
        Some((row, is_bound))
    }

    fn tuple_string_row(
        field: StringField,
        ctx: &TupleRowContext<'_>,
    ) -> Option<(gpui::AnyElement, bool)> {
        let input = ctx.inputs.get(&field.target.key)?.clone();
        let binding = Self::scene_field_binding(
            ctx.editing_scene,
            false,
            field.scene_bindable,
            SceneBindingTarget::new(
                ctx.item_id,
                SceneBindingOwner::from_effect(field.target.effect_id),
                field.target.parameter_id.clone(),
                field.target.value_path,
            ),
            &ParameterType::Value(ParameterValueType::Scalar(ScalarParameterType::String)),
            ctx.scene_arguments,
        );
        let is_bound = binding
            .as_ref()
            .is_some_and(|binding| binding.connected.is_some());
        let binding_button = binding.map(|binding| {
            Self::scene_binding_button(
                binding,
                ctx.inspector,
                SharedString::from(format!("bind-scene-argument-{}", field.target.key)),
            )
        });
        let row = div()
            .min_w_0()
            .w_full()
            .flex()
            .items_center()
            .gap_2()
            .when_some(
                Self::tuple_element_label(field.element_label.clone()),
                |this, label| this.child(label),
            )
            .child(
                div()
                    .min_w_0()
                    .flex_1()
                    .flex()
                    .items_center()
                    .gap_1()
                    .when(!is_bound, |this| {
                        this.child(
                            Input::new(&input)
                                .small()
                                .w_full()
                                .when(field.multiline, |input| input.h(px(72.))),
                        )
                    })
                    .when_some(binding_button, |this, button| this.child(button)),
            )
            .into_any_element();
        Some((row, is_bound))
    }

    fn tuple_choice_row(field: ChoiceField, ctx: &TupleRowContext<'_>) -> (gpui::AnyElement, bool) {
        let binding = Self::scene_field_binding(
            ctx.editing_scene,
            false,
            field.scene_bindable,
            SceneBindingTarget::new(
                ctx.item_id,
                SceneBindingOwner::from_effect(field.target.effect_id),
                field.target.parameter_id.clone(),
                field.target.value_path,
            ),
            &field.ty,
            ctx.scene_arguments,
        );
        let is_bound = binding
            .as_ref()
            .is_some_and(|binding| binding.connected.is_some());
        let binding_button = binding.map(|binding| {
            Self::scene_binding_button(
                binding,
                ctx.inspector,
                SharedString::from(format!("bind-{}", field.target.key)),
            )
        });
        let value_path = field.target.value_path;
        let parameter_id = field.target.parameter_id.clone();
        let effect_id = field.target.effect_id;
        let selected_label = field
            .options
            .iter()
            .find(|(_, value)| *value == field.value)
            .map(|(label, _)| label.clone())
            .unwrap_or_default();
        let options = field.options.clone();
        let editor = ctx.editor.clone();
        let row = div()
            .min_w_0()
            .w_full()
            .flex()
            .items_center()
            .gap_2()
            .when_some(
                Self::tuple_element_label(field.element_label.clone()),
                |this, label| this.child(label),
            )
            .child(
                div()
                    .min_w_0()
                    .flex_1()
                    .flex()
                    .items_center()
                    .gap_1()
                    .when(!is_bound, |this| {
                        this.child(
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
                    .when_some(binding_button, |this, button| this.child(button)),
            )
            .into_any_element();
        (row, is_bound)
    }

    fn tuple_bool_row(field: BoolField, ctx: &TupleRowContext<'_>) -> (gpui::AnyElement, bool) {
        let binding = Self::scene_field_binding(
            ctx.editing_scene,
            false,
            field.scene_bindable,
            SceneBindingTarget::new(
                ctx.item_id,
                SceneBindingOwner::from_effect(field.target.effect_id),
                field.target.parameter_id.clone(),
                field.target.value_path,
            ),
            &ParameterType::Value(ParameterValueType::Scalar(ScalarParameterType::Bool)),
            ctx.scene_arguments,
        );
        let is_bound = binding
            .as_ref()
            .is_some_and(|binding| binding.connected.is_some());
        let binding_button = binding.map(|binding| {
            Self::scene_binding_button(
                binding,
                ctx.inspector,
                SharedString::from(format!("bind-scene-argument-{}", field.target.key)),
            )
        });
        let value_path = field.target.value_path;
        let parameter_id = field.target.parameter_id.clone();
        let effect_id = field.target.effect_id;
        let checked = field.value && !field.mixed;
        let mixed = field.mixed;
        let editor = ctx.editor.clone();
        let row = div()
            .min_w_0()
            .w_full()
            .flex()
            .items_center()
            .gap_2()
            .when_some(
                Self::tuple_element_label(field.element_label.clone()),
                |this, label| this.child(label),
            )
            .child(
                div()
                    .min_w_0()
                    .flex_1()
                    .flex()
                    .items_center()
                    .gap_2()
                    .when(!is_bound && mixed, |this| {
                        this.child(div().text_xs().child("混在"))
                    })
                    .when(!is_bound, |this| {
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
            .into_any_element();
        (row, is_bound)
    }

    fn tuple_color_row(
        field: ColorField,
        ctx: &TupleRowContext<'_>,
    ) -> Option<(gpui::AnyElement, bool)> {
        let picker = ctx.color_pickers.get(&field.target.key)?.clone();
        let animation_enabled = field.target.animation_enabled(ctx.item);
        let binding = Self::scene_field_binding(
            ctx.editing_scene,
            animation_enabled,
            field.scene_bindable,
            SceneBindingTarget::new(
                ctx.item_id,
                SceneBindingOwner::from_effect(field.target.effect_id),
                field.target.parameter_id.clone(),
                field.target.value_path,
            ),
            &ParameterType::Value(ParameterValueType::Scalar(ScalarParameterType::Color)),
            ctx.scene_arguments,
        );
        let is_bound = binding
            .as_ref()
            .is_some_and(|binding| binding.connected.is_some());
        let binding_button = binding.map(|binding| {
            Self::scene_binding_button(
                binding,
                ctx.inspector,
                SharedString::from(format!("bind-scene-argument-{}", field.target.key)),
            )
        });
        let animation_button = (field.animatable && !is_bound).then(|| {
            let inspector = ctx.inspector.clone();
            let property = field.target.clone();
            Button::new(SharedString::from(format!(
                "toggle-animation-{}",
                field.target.key
            )))
            .icon(Icon::new(IconName::Keyframe))
            .small()
            .compact()
            .ghost()
            .selected(animation_enabled)
            .tooltip(if animation_enabled {
                "アニメーションを解除"
            } else {
                "色全体をアニメーションする"
            })
            .on_click(move |_, window, cx| {
                inspector.update(cx, |inspector, cx| {
                    inspector.set_animation_enabled(&property, !animation_enabled, window, cx);
                });
            })
        });
        let animated = ctx
            .animation_color_pickers
            .get(&(field.target.key.clone(), AnimationEndpoint::From))
            .zip(
                ctx.animation_color_pickers
                    .get(&(field.target.key.clone(), AnimationEndpoint::To)),
            )
            .filter(|_| animation_enabled);
        let value = match animated {
            Some((from, to)) => div()
                .min_w_0()
                .flex()
                .flex_1()
                .gap_1()
                .child(
                    div()
                        .min_w_0()
                        .flex_1()
                        .flex()
                        .flex_col()
                        .gap_1()
                        .child(div().text_xs().child("開始"))
                        .child(ColorPicker::new(from).small().w_full()),
                )
                .child(
                    div()
                        .min_w_0()
                        .flex_1()
                        .flex()
                        .flex_col()
                        .gap_1()
                        .child(div().text_xs().child("終了"))
                        .child(ColorPicker::new(to).small().w_full()),
                )
                .into_any_element(),
            None => ColorPicker::new(&picker)
                .small()
                .w_full()
                .into_any_element(),
        };
        let select_inspector = ctx.inspector.clone();
        let select_property = field.target.clone();
        let row = div()
            .min_w_0()
            .w_full()
            .flex()
            .items_center()
            .gap_2()
            .when_some(
                Self::tuple_element_label(field.element_label.clone()),
                |this, label| this.child(label),
            )
            .child(
                div()
                    .min_w_0()
                    .flex_1()
                    .flex()
                    .items_center()
                    .gap_1()
                    .when(!is_bound, |this| {
                        this.child(
                            div()
                                .min_w_0()
                                .flex_1()
                                .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                                    select_inspector.update(cx, |inspector, cx| {
                                        inspector.select_animation(&select_property, cx);
                                    });
                                })
                                .child(value),
                        )
                    })
                    .when_some(animation_button, |this, button| this.child(button))
                    .when_some(binding_button, |this, button| this.child(button)),
            )
            .into_any_element();
        Some((row, is_bound))
    }

    /// Render one tuple element control as compact rows, returning each row
    /// with whether its scene argument is connected.
    pub(super) fn tuple_control_rows(
        control: PropertyControl,
        ctx: &TupleRowContext<'_>,
    ) -> Vec<(gpui::AnyElement, bool)> {
        match control {
            PropertyControl::Number(field) => {
                Self::tuple_number_row(field, ctx).into_iter().collect()
            }
            PropertyControl::String(field) => {
                Self::tuple_string_row(field, ctx).into_iter().collect()
            }
            PropertyControl::Choice(field) => vec![Self::tuple_choice_row(field, ctx)],
            PropertyControl::Bool(field) => vec![Self::tuple_bool_row(field, ctx)],
            PropertyControl::Color(field) => {
                Self::tuple_color_row(field, ctx).into_iter().collect()
            }
            PropertyControl::Array(_) | PropertyControl::Tuple(_) => Vec::new(),
        }
    }

    pub(super) fn tuple_field_element(
        tuple: TupleField,
        view: &InspectorSelectionView,
        render: &InspectorRenderContext<'_>,
    ) -> gpui::AnyElement {
        let size_key = tuple.controls.iter().find_map(|control| match control {
            PropertyControl::Number(field) if field.target.effect_id.is_none() && field.is_size => {
                Some(field.target.key.clone())
            }
            _ => None,
        });
        let aspect = size_key.zip(view.aspect_ratio_lock);
        let aspect_row = aspect.as_ref().map(|(key, state)| {
            div()
                .w_full()
                .flex()
                .items_center()
                .justify_end()
                .gap_2()
                .child(Self::aspect_ratio_control(
                    key.clone(),
                    *state,
                    render.colors.muted_foreground,
                    render.editor,
                ))
        });
        let ctx = TupleRowContext {
            item_id: view.item.id,
            editing_scene: view.editing_scene,
            scene_arguments: &view.scene_arguments,
            item: &view.item,
            inputs: render.inputs,
            animation_inputs: render.animation_inputs,
            color_pickers: render.color_pickers,
            animation_color_pickers: render.animation_color_pickers,
            editor: render.editor,
            inspector: &render.inspector,
            focus_handle: render.focus_handle,
            size_locked: aspect.as_ref().is_some_and(|(_, state)| state.checked()),
            muted_color: Some(render.colors.muted_foreground),
        };
        let rows = tuple
            .controls
            .into_iter()
            .flat_map(|control| {
                Self::tuple_control_rows(control, &ctx)
                    .into_iter()
                    .map(|(row, _)| row)
            })
            .collect::<Vec<_>>();
        div()
            .w_full()
            .flex()
            .items_start()
            .gap_3()
            .child(Self::parameter_label_column(tuple.label))
            .child(
                div()
                    .w_0()
                    .min_w_0()
                    .flex()
                    .flex_1()
                    .flex_col()
                    .gap_1()
                    .when_some(aspect_row, |this, row| this.child(row))
                    .children(rows),
            )
            .into_any_element()
    }
}
