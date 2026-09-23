use super::*;

impl PropertyInspector {
    pub(super) fn choice_dropdown(
        target: &PropertyTarget,
        current: u32,
        options: &[(String, u32)],
        read_only: bool,
        inspector: &Entity<Self>,
    ) -> impl IntoElement {
        let selected_label = options
            .iter()
            .find(|(_, value)| *value == current)
            .map(|(label, _)| label.clone())
            .unwrap_or_default();
        let options = options.to_vec();
        let inspector = inspector.clone();
        let target = target.clone();
        Button::new(SharedString::from(target.key.to_string()))
            .small()
            .w_full()
            .disabled(read_only)
            .label(selected_label)
            .dropdown_caret(true)
            .popup_menu(move |menu, _, _| {
                options.iter().fold(menu, |menu, (label, value)| {
                    let inspector = inspector.clone();
                    let target = target.clone();
                    let value = *value;
                    menu.item(PopupMenuItem::new(label.clone()).on_click(move |_, _, cx| {
                        inspector.update(cx, |inspector, cx| {
                            let changed =
                                inspector.set_scalar(&target, PropertyValue::Enum(value), cx);
                            if changed {
                                cx.notify();
                            }
                        });
                    }))
                })
            })
    }

    pub(super) fn bool_switch(
        target: &PropertyTarget,
        value: bool,
        mixed: bool,
        read_only: bool,
        inspector: &Entity<Self>,
    ) -> (Switch, bool) {
        let checked = value && !mixed;
        let inspector = inspector.clone();
        let target = target.clone();
        let switch = Switch::new(SharedString::from(target.key.to_string()))
            .small()
            .checked(checked)
            .disabled(read_only)
            .tooltip(if mixed {
                "値が混在しています。クリックですべてオン"
            } else if checked {
                "オフにする"
            } else {
                "オンにする"
            })
            .on_click(move |checked, _, cx| {
                inspector.update(cx, |inspector, cx| {
                    let changed = inspector.set_scalar(&target, PropertyValue::Bool(*checked), cx);
                    if changed {
                        cx.notify();
                    }
                });
            });
        (switch, mixed)
    }

    pub(super) fn draggable_number_input(
        target: &PropertyTarget,
        spec: &NumberSpec,
        input: &Entity<InputState>,
        presentation: DraggableNumberInput,
        disabled: bool,
        ctx: &RenderCtx,
    ) -> gpui::AnyElement {
        let DraggableNumberInput {
            id: input_id,
            animation_stop,
        } = presentation;
        let is_animation_stop = animation_stop.is_some();
        let element_id = SharedString::from(format!("value-drag-{input_id:?}"));
        let drag = PropertyValueDrag {
            inspector_id: ctx.inspector.entity_id(),
            input_id: input_id.clone(),
        };
        let drag_inspector = ctx.inspector.clone();
        let drag_target = target.clone();
        let drag_spec = spec.clone();
        let drag_input_id = input_id;
        let drag_animation_stop = animation_stop;
        let drag_input = input.clone();
        let value_input = NumberInput::new(input)
            .small()
            .w_full()
            .min_w_0()
            .when(is_animation_stop, |input| {
                input.min_w(px(Self::ANIMATION_STOP_INPUT_MIN_WIDTH))
            })
            .disabled(disabled)
            .suffix(div().text_sm().child(spec.suffix.clone()));

        div()
            .id(element_id)
            .w_full()
            .min_w_0()
            .flex()
            .when(!disabled, |this| {
                this.on_mouse_down(MouseButton::Left, move |event, _, cx| {
                    drag_inspector.update(cx, |inspector, cx| {
                        inspector.prepare_value_drag(
                            &drag_target,
                            &drag_spec,
                            &drag_input_id,
                            drag_animation_stop.clone(),
                            event,
                            cx,
                        );
                    });
                })
            })
            .when(!disabled, |this| {
                this.on_drag(drag, move |drag, _, window, cx| {
                    cx.stop_propagation();
                    drag_input.update(cx, |input, cx| input.unselect(window, cx));
                    cx.new(|_| drag.clone())
                })
            })
            .child(value_input)
            .into_any_element()
    }

    pub(super) fn aspect_ratio_control(
        key: InspectorPath,
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

    pub(super) fn number_editor(
        common: &LeafControl,
        spec: &NumberSpec,
        input: &Entity<InputState>,
        disabled: bool,
        ctx: &RenderCtx,
    ) -> gpui::AnyElement {
        let mut stop_inputs = common
            .animation_stops
            .iter()
            .filter_map(|stop| {
                let input = ctx.store.text(&stop.id)?;
                Some(Self::draggable_number_input(
                    &common.target,
                    spec,
                    &input,
                    DraggableNumberInput {
                        id: stop.id.clone(),
                        animation_stop: Some(AnimationStopBinding::new(
                            ctx.item_id,
                            common.target.effect_id,
                            stop,
                        )),
                    },
                    disabled,
                    ctx,
                ))
            })
            .collect::<Vec<_>>();
        if stop_inputs.len() == 1 {
            return stop_inputs.remove(0);
        }
        if stop_inputs.len() == 2 {
            let end = stop_inputs.pop().expect("two stop inputs");
            let start = stop_inputs.pop().expect("two stop inputs");
            return div()
                .min_w_0()
                .flex_1()
                .flex()
                .items_center()
                .gap_1()
                .child(
                    div()
                        .min_w(px(Self::ANIMATION_STOP_INPUT_MIN_WIDTH))
                        .flex_1()
                        .child(start),
                )
                .child(
                    Icon::new(IconName::ArrowRight)
                        .xsmall()
                        .text_color(ctx.colors.muted_foreground),
                )
                .child(
                    div()
                        .min_w(px(Self::ANIMATION_STOP_INPUT_MIN_WIDTH))
                        .flex_1()
                        .child(end),
                )
                .into_any_element();
        }
        Self::draggable_number_input(
            &common.target,
            spec,
            input,
            DraggableNumberInput {
                id: common.id.clone(),
                animation_stop: None,
            },
            disabled,
            ctx,
        )
    }

    pub(super) fn text_editor(
        input: &Entity<InputState>,
        multiline: bool,
        read_only: bool,
    ) -> impl IntoElement {
        Input::new(input)
            .small()
            .w_full()
            .disabled(read_only)
            .when(multiline, |input| input.h(px(72.)))
    }

    pub(super) fn color_editor(
        common: &LeafControl,
        picker: &Entity<ColorPickerState>,
        ctx: &RenderCtx,
    ) -> gpui::AnyElement {
        if common.read_only {
            let color = match common.value {
                PropertyValue::Color(color) => Self::color_to_hsla(color),
                _ => ctx.colors.background,
            };
            return div()
                .w_full()
                .h(px(28.))
                .flex()
                .items_center()
                .child(
                    div()
                        .size(px(24.))
                        .rounded_md()
                        .border_1()
                        .border_color(ctx.colors.border)
                        .bg(color)
                        .opacity(0.55),
                )
                .into_any_element();
        }
        let stop_inputs = common
            .animation_stops
            .iter()
            .filter_map(|stop| ctx.store.color(&stop.id))
            .collect::<Vec<_>>();
        if let [picker] = stop_inputs.as_slice() {
            return ColorPicker::new(picker).small().w_full().into_any_element();
        }
        if let [start, end] = stop_inputs.as_slice() {
            return div()
                .min_w_0()
                .flex_1()
                .flex()
                .items_center()
                .gap_1()
                .child(
                    div()
                        .min_w_0()
                        .flex_1()
                        .child(ColorPicker::new(start).small().w_full()),
                )
                .child(
                    Icon::new(IconName::ArrowRight)
                        .xsmall()
                        .text_color(ctx.colors.muted_foreground),
                )
                .child(
                    div()
                        .min_w_0()
                        .flex_1()
                        .child(ColorPicker::new(end).small().w_full()),
                )
                .into_any_element();
        }
        ColorPicker::new(picker).small().w_full().into_any_element()
    }
}
