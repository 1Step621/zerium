use super::*;

impl PropertyInspector {
    pub(super) fn animated_number_input(
        field: &NumberField,
        endpoint: AnimationEndpoint,
        input: &Entity<InputState>,
        inspector: &Entity<Self>,
        focus_handle: &FocusHandle,
        show_suffix: bool,
        disabled: bool,
    ) -> gpui::AnyElement {
        let drag = PropertyValueDrag {
            inspector_id: inspector.entity_id(),
            path: field.target.key.clone(),
            animation_endpoint: Some(endpoint),
        };
        let drag_inspector = inspector.clone();
        let drag_field = field.clone();
        let drag_input = input.clone();
        let drag_focus_handle = focus_handle.clone();
        let number_input = NumberInput::new(input).small().w_full().disabled(disabled);
        let number_input = if show_suffix {
            number_input.suffix(div().text_sm().child(field.input.suffix.clone()))
        } else {
            number_input
        };
        div()
            .id(SharedString::from(format!(
                "animation-drag-{}/{}",
                field.target.key,
                match endpoint {
                    AnimationEndpoint::From => "from",
                    AnimationEndpoint::To => "to",
                }
            )))
            .w_0()
            .min_w_0()
            .flex()
            .flex_1()
            .when(!disabled, |this| {
                this.on_mouse_down(MouseButton::Left, move |event, _, cx| {
                    drag_inspector.update(cx, |inspector, cx| {
                        inspector.prepare_value_drag(&drag_field, event, Some(endpoint), cx);
                    });
                })
            })
            .when(!disabled, |this| {
                this.on_drag(drag, move |drag, _, window, cx| {
                    cx.stop_propagation();
                    drag_input.update(cx, |input, cx| input.unselect(window, cx));
                    drag_focus_handle.focus(window, cx);
                    cx.new(|_| drag.clone())
                })
            })
            .child(number_input)
            .into_any_element()
    }

    pub(super) fn draggable_number_input(
        field: &NumberField,
        input: &Entity<InputState>,
        inspector: &Entity<Self>,
        focus_handle: &FocusHandle,
        disabled: bool,
    ) -> gpui::AnyElement {
        let drag = PropertyValueDrag {
            inspector_id: inspector.entity_id(),
            path: field.target.key.clone(),
            animation_endpoint: None,
        };
        let drag_inspector = inspector.clone();
        let drag_field = field.clone();
        let drag_input = input.clone();
        let drag_focus_handle = focus_handle.clone();

        div()
            .id(SharedString::from(format!(
                "value-drag-{}",
                field.target.key
            )))
            .w_full()
            .min_w_0()
            .flex()
            .when(!disabled, |this| {
                this.on_mouse_down(MouseButton::Left, move |event, _, cx| {
                    drag_inspector.update(cx, |inspector, cx| {
                        inspector.prepare_value_drag(&drag_field, event, None, cx);
                    });
                })
            })
            .when(!disabled, |this| {
                this.on_drag(drag, move |drag, _, window, cx| {
                    cx.stop_propagation();
                    drag_input.update(cx, |input, cx| input.unselect(window, cx));
                    drag_focus_handle.focus(window, cx);
                    cx.new(|_| drag.clone())
                })
            })
            .child(
                NumberInput::new(input)
                    .small()
                    .w_full()
                    .disabled(disabled)
                    .suffix(div().text_sm().child(field.input.suffix.clone())),
            )
            .into_any_element()
    }

    pub(super) fn aspect_ratio_control(
        key: PropertyPath,
        state: AspectRatioLockState,
        muted_color: gpui::Hsla,
        editor: &Entity<TimelineEditor>,
    ) -> Div {
        let editor = editor.clone();
        let checked = state.checked();
        div()
            .flex()
            .items_center()
            .gap_2()
            .child(div().text_xs().text_color(muted_color).child("比率固定"))
            .child(
                Switch::new(SharedString::from(format!("aspect-ratio-lock-{key}")))
                    .small()
                    .checked(checked)
                    .disabled(state.disabled_by_scene_size_argument)
                    .tooltip(if state.disabled_by_scene_size_argument {
                        "サイズが引数接続されているため一時的に無効"
                    } else if state.mixed {
                        "ロック状態が混在しています。クリックですべてオン"
                    } else if state.multiple {
                        "各アイテムの現在の縦横比を個別に固定"
                    } else if checked {
                        "アスペクト比維持を解除"
                    } else {
                        "現在のアスペクト比を維持"
                    })
                    .on_click(move |checked, _, cx| {
                        editor.update(cx, |editor, cx| {
                            if editor.update_selected_aspect_ratio_locked(*checked) {
                                cx.notify();
                            }
                        });
                    }),
            )
    }

    pub(super) fn property_row(
        field: NumberField,
        input: &Entity<InputState>,
        animation_inputs: Option<&(Entity<InputState>, Entity<InputState>)>,
        animation_enabled: bool,
        scene_binding: Option<SceneFieldBinding>,
        inspector: &Entity<Self>,
        focus_handle: &FocusHandle,
    ) -> impl IntoElement + use<> {
        let is_scene_bound = scene_binding
            .as_ref()
            .is_some_and(|binding| binding.connected.is_some());
        let select_inspector = inspector.clone();
        let select_field = field.clone();
        let row_id = SharedString::from(field.target.key.to_string());
        let animation_button = (field.animatable && !is_scene_bound).then(|| {
            let animation_inspector = inspector.clone();
            let animation_field = field.clone();
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
                "アニメーションする"
            })
            .on_click(move |_, window, cx| {
                animation_inspector.update(cx, |inspector, cx| {
                    inspector.set_number_animation_enabled(
                        &animation_field,
                        !animation_enabled,
                        window,
                        cx,
                    );
                });
            })
        });
        let publish_button = scene_binding.map(|binding| {
            Self::scene_binding_button(
                binding,
                inspector,
                SharedString::from(format!("bind-scene-argument-{}", field.target.key)),
            )
        });
        let value_input = if let Some((from, to)) = animation_inputs.filter(|_| animation_enabled) {
            div()
                .w_full()
                .flex()
                .gap_1()
                .child(Self::animated_number_input(
                    &field,
                    AnimationEndpoint::From,
                    from,
                    inspector,
                    focus_handle,
                    false,
                    false,
                ))
                .child(Self::animated_number_input(
                    &field,
                    AnimationEndpoint::To,
                    to,
                    inspector,
                    focus_handle,
                    true,
                    false,
                ))
                .into_any_element()
        } else {
            Self::draggable_number_input(&field, input, inspector, focus_handle, false)
        };

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
                    .gap_1()
                    .when(!is_scene_bound, |this| {
                        this.child(
                            div()
                                .id(row_id)
                                .min_w_0()
                                .flex()
                                .flex_1()
                                .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                                    select_inspector.update(cx, |this, cx| {
                                        this.select_number_animation(&select_field, cx);
                                    });
                                })
                                .child(value_input),
                        )
                    })
                    .when_some(animation_button, |this, button| this.child(button))
                    .when_some(publish_button, |this, button| this.child(button)),
            )
    }
}
