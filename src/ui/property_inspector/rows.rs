use super::control::{Control, ElementGroup, ElementKind, LeafControl, NumberControl, NumberSpec};
use super::state::ControlStore;
use super::*;

pub(super) struct RenderCtx<'a> {
    pub colors: ThemeColor,
    pub editor: &'a Entity<TimelineEditor>,
    pub inspector: Entity<PropertyInspector>,
    pub store: &'a ControlStore,
    pub font_names: &'a [String],
    pub item_id: ItemId,
    pub active_scene_name_input: Option<Entity<InputState>>,
}

struct DraggableNumberInput {
    id: ControlId,
    animation_stop: Option<AnimationStopBinding>,
}

impl PropertyInspector {
    fn scalar_label(label: Option<String>) -> Option<Div> {
        label.map(|label| div().w(px(32.)).flex_none().text_sm().child(label))
    }

    // Layout primitives. Editors below only build their value widget and
    // these helpers provide the shared labeled/compact row geometry.
    fn labeled_row(label: impl Into<SharedString>, content: Div) -> Div {
        div()
            .w_full()
            .flex()
            .items_center()
            .gap_3()
            .child(Self::property_label_column(label))
            .child(content)
    }

    fn compact_row(label: Option<String>, content: Div) -> Div {
        div()
            .min_w_0()
            .w_full()
            .flex()
            .items_center()
            .gap_2()
            .when_some(Self::scalar_label(label), |this, label| this.child(label))
            .child(content)
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

    fn keyframe_base(id: SharedString, selected: bool, tooltip: String) -> Button {
        Button::new(id)
            .icon(Icon::new(IconName::Keyframe))
            .small()
            .compact()
            .ghost()
            .selected(selected)
            .tooltip(tooltip)
    }

    fn number_animation_toggle(
        target: &PropertyTarget,
        spec: &NumberSpec,
        enabled: bool,
        tooltip: String,
        inspector: &Entity<Self>,
    ) -> Button {
        let inspector = inspector.clone();
        let target = target.clone();
        let spec = spec.clone();
        Self::keyframe_base(
            SharedString::from(format!("toggle-animation-{}", target.key)),
            enabled,
            tooltip,
        )
        .on_click(move |_, window, cx| {
            inspector.update(cx, |inspector, cx| {
                inspector.set_number_animation_enabled(&target, &spec, !enabled, window, cx);
            });
        })
    }

    fn coordinate_animation_toggle(
        target: &PropertyTarget,
        spec: &NumberSpec,
        enabled: bool,
        disabled: bool,
        visible: bool,
        inspector: &Entity<Self>,
    ) -> Button {
        let inspector = inspector.clone();
        let target = target.clone();
        let spec = spec.clone();
        Self::keyframe_base(
            SharedString::from(format!("toggle-animation-{}", target.key)),
            visible,
            if disabled {
                "アスペクト比維持中は幅から自動計算".to_owned()
            } else if enabled {
                "この座標のアニメーションを解除".to_owned()
            } else {
                "この座標をアニメーションする".to_owned()
            },
        )
        .tab_stop(!disabled)
        .when(!disabled, |button| {
            button.on_click(move |_, window, cx| {
                cx.stop_propagation();
                inspector.update(cx, |inspector, cx| {
                    inspector.set_number_animation_enabled(&target, &spec, !enabled, window, cx);
                });
            })
        })
    }

    fn color_animation_toggle(
        target: &PropertyTarget,
        enabled: bool,
        inspector: &Entity<Self>,
    ) -> Button {
        let inspector = inspector.clone();
        let target = target.clone();
        Self::keyframe_base(
            SharedString::from(format!("toggle-animation-{}", target.key)),
            enabled,
            if enabled {
                "アニメーションを解除".to_owned()
            } else {
                "色全体をアニメーションする".to_owned()
            },
        )
        .on_click(move |_, window, cx| {
            inspector.update(cx, |inspector, cx| {
                inspector.set_animation_enabled(&target, !enabled, window, cx);
            });
        })
    }

    fn choice_dropdown(
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

    fn bool_switch(
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

    fn draggable_number_input(
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

    fn number_editor(
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

    fn text_editor(
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

    fn color_editor(
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

    fn number_full_row(
        common: &LeafControl,
        spec: &NumberSpec,
        input: &Entity<InputState>,
        animation_enabled: bool,
        binding: Option<SceneFieldBinding>,
        ctx: &RenderCtx,
    ) -> Div {
        let mut label = common.label.clone();
        if let Some(scalar_label) = common.scalar_label.clone() {
            label = format!("{label} {scalar_label}");
        }
        let is_bound = binding
            .as_ref()
            .is_some_and(|binding| binding.connected.is_some());
        let select_inspector = ctx.inspector.clone();
        let select_target = common.target.clone();
        let select_spec = spec.clone();
        let animation_button = (common.animatable && !is_bound).then(|| {
            Self::number_animation_toggle(
                &common.target,
                spec,
                animation_enabled,
                if animation_enabled {
                    "アニメーションを解除".to_owned()
                } else {
                    "アニメーションする".to_owned()
                },
                &ctx.inspector,
            )
        });
        let publish_button = binding.map(|binding| {
            Self::scene_binding_button(
                binding,
                &ctx.inspector,
                SharedString::from(format!("bind-scene-argument-{}", common.target.key)),
            )
        });
        let value_input = Self::number_editor(common, spec, input, common.read_only, ctx);
        div()
            .w_full()
            .flex()
            .items_center()
            .gap_3()
            .child(Self::property_label_column(label))
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
                                .id(SharedString::from(common.target.key.to_string()))
                                .min_w_0()
                                .flex()
                                .flex_1()
                                .when(!common.read_only, |this| {
                                    this.on_mouse_down(MouseButton::Left, move |_, _, cx| {
                                        select_inspector.update(cx, |inspector, cx| {
                                            inspector.select_number_animation(
                                                &select_target,
                                                &select_spec,
                                                cx,
                                            );
                                        });
                                    })
                                })
                                .child(value_input),
                        )
                    })
                    .when_some(animation_button, |this, button| this.child(button))
                    .when_some(publish_button, |this, button| this.child(button)),
            )
    }

    fn number_compact_row(
        common: &LeafControl,
        spec: &NumberSpec,
        input: &Entity<InputState>,
        size_locked: bool,
        ctx: &RenderCtx,
    ) -> (gpui::AnyElement, bool) {
        let coordinate_animation_enabled = common.animation_enabled;
        let binding = common.binding.clone();
        let is_bound = binding
            .as_ref()
            .is_some_and(|binding| binding.connected.is_some());
        let binding_button = binding.map(|binding| {
            Self::scene_binding_button(
                binding,
                &ctx.inspector,
                SharedString::from(format!("bind-scene-argument-{}", common.target.key)),
            )
        });
        let disabled =
            common.read_only || size_locked && common.target.path.scalar_index() == Some(1);
        let animation_visible = coordinate_animation_enabled;
        let animation_button = (common.animatable && !is_bound).then(|| {
            Self::coordinate_animation_toggle(
                &common.target,
                spec,
                coordinate_animation_enabled,
                disabled,
                animation_visible,
                &ctx.inspector,
            )
        });
        let value_input = Self::number_editor(common, spec, input, disabled, ctx);
        let select_inspector = ctx.inspector.clone();
        let select_target = common.target.clone();
        let select_spec = spec.clone();
        let row = div()
            .min_w_0()
            .w_full()
            .flex()
            .items_center()
            .gap_2()
            .when(disabled, |this| {
                this.text_color(ctx.colors.muted_foreground)
            })
            .when(!common.read_only, |this| {
                this.on_mouse_down(MouseButton::Left, move |_, _, cx| {
                    select_inspector.update(cx, |inspector, cx| {
                        inspector.select_number_animation(&select_target, &select_spec, cx);
                    });
                })
            })
            .when_some(
                Self::scalar_label(common.scalar_label.clone()),
                |this, label| this.child(label),
            )
            .when(!is_bound, |this| {
                this.child(div().min_w_0().flex().flex_1().child(value_input))
            })
            .when_some(animation_button, |this, button| this.child(button))
            .when_some(binding_button, |this, button| this.child(button))
            .into_any_element();
        (row, is_bound)
    }

    fn text_full_row(
        common: &LeafControl,
        multiline: bool,
        input: &Entity<InputState>,
        binding: Option<SceneFieldBinding>,
        ctx: &RenderCtx,
    ) -> Div {
        let is_bound = binding
            .as_ref()
            .is_some_and(|binding| binding.connected.is_some());
        let binding_button = binding.map(|binding| {
            Self::scene_binding_button(
                binding,
                &ctx.inspector,
                SharedString::from(format!("bind-scene-argument-{}", common.target.key)),
            )
        });
        Self::labeled_row(
            common.label.clone(),
            div()
                .min_w_0()
                .flex_1()
                .flex()
                .items_center()
                .gap_1()
                .when(!is_bound, |this| {
                    this.child(Self::text_editor(input, multiline, common.read_only))
                })
                .when_some(binding_button, |this, button| this.child(button)),
        )
    }

    fn text_compact_row(
        common: &LeafControl,
        multiline: bool,
        input: &Entity<InputState>,
        binding: Option<SceneFieldBinding>,
        ctx: &RenderCtx,
    ) -> (gpui::AnyElement, bool) {
        let is_bound = binding
            .as_ref()
            .is_some_and(|binding| binding.connected.is_some());
        let binding_button = binding.map(|binding| {
            Self::scene_binding_button(
                binding,
                &ctx.inspector,
                SharedString::from(format!("bind-scene-argument-{}", common.target.key)),
            )
        });
        let row = Self::compact_row(
            common.scalar_label.clone(),
            div()
                .min_w_0()
                .flex_1()
                .flex()
                .items_center()
                .gap_1()
                .when(!is_bound, |this| {
                    this.child(Self::text_editor(input, multiline, common.read_only))
                })
                .when_some(binding_button, |this, button| this.child(button)),
        )
        .into_any_element();
        (row, is_bound)
    }

    fn toggle_full_row(
        common: &LeafControl,
        value: bool,
        mixed: bool,
        binding: Option<SceneFieldBinding>,
        ctx: &RenderCtx,
    ) -> gpui::AnyElement {
        let is_bound = binding
            .as_ref()
            .is_some_and(|binding| binding.connected.is_some());
        let binding_button = binding.map(|binding| {
            Self::scene_binding_button(
                binding,
                &ctx.inspector,
                SharedString::from(format!("bind-scene-argument-{}", common.target.key)),
            )
        });
        let (switch, mixed) = Self::bool_switch(
            &common.target,
            value,
            mixed,
            common.read_only,
            &ctx.inspector,
        );
        Self::labeled_row(
            common.label.clone(),
            div()
                .min_w_0()
                .flex_1()
                .flex()
                .items_center()
                .gap_2()
                .when(!is_bound && mixed, |this| {
                    this.child(div().text_xs().child("混在"))
                })
                .when(!is_bound, |this| this.child(switch))
                .when_some(binding_button, |this, button| this.child(button)),
        )
        .into_any_element()
    }

    fn toggle_compact_row(
        common: &LeafControl,
        value: bool,
        mixed: bool,
        binding: Option<SceneFieldBinding>,
        ctx: &RenderCtx,
    ) -> (gpui::AnyElement, bool) {
        let is_bound = binding
            .as_ref()
            .is_some_and(|binding| binding.connected.is_some());
        let binding_button = binding.map(|binding| {
            Self::scene_binding_button(
                binding,
                &ctx.inspector,
                SharedString::from(format!("bind-scene-argument-{}", common.target.key)),
            )
        });
        let (switch, mixed) = Self::bool_switch(
            &common.target,
            value,
            mixed,
            common.read_only,
            &ctx.inspector,
        );
        let row = div()
            .min_w_0()
            .w_full()
            .flex()
            .items_center()
            .gap_2()
            .when_some(
                Self::scalar_label(common.scalar_label.clone()),
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
                    .when(!is_bound, |this| this.child(switch))
                    .when_some(binding_button, |this, button| this.child(button)),
            )
            .into_any_element();
        (row, is_bound)
    }

    fn dropdown_full_row(
        common: &LeafControl,
        current: u32,
        options: &[(String, u32)],
        binding: Option<SceneFieldBinding>,
        ctx: &RenderCtx,
    ) -> gpui::AnyElement {
        let bound = binding
            .as_ref()
            .is_some_and(|binding| binding.connected.is_some());
        let binding_button = binding.map(|binding| {
            Self::scene_binding_button(
                binding,
                &ctx.inspector,
                SharedString::from(format!("bind-{}", common.target.key)),
            )
        });
        div()
            .w_full()
            .flex()
            .items_center()
            .gap_3()
            .child(Self::property_label_column(common.label.clone()))
            .when(!bound, |row| {
                row.child(Self::choice_dropdown(
                    &common.target,
                    current,
                    options,
                    common.read_only,
                    &ctx.inspector,
                ))
            })
            .when_some(binding_button, |row, button| row.child(button))
            .into_any_element()
    }

    fn dropdown_compact_row(
        common: &LeafControl,
        current: u32,
        options: &[(String, u32)],
        binding: Option<SceneFieldBinding>,
        ctx: &RenderCtx,
    ) -> (gpui::AnyElement, bool) {
        let bound = binding
            .as_ref()
            .is_some_and(|binding| binding.connected.is_some());
        let binding_button = binding.map(|binding| {
            Self::scene_binding_button(
                binding,
                &ctx.inspector,
                SharedString::from(format!("bind-{}", common.target.key)),
            )
        });
        let row = div()
            .min_w_0()
            .w_full()
            .flex()
            .items_center()
            .gap_2()
            .when_some(
                Self::scalar_label(common.scalar_label.clone()),
                |this, label| this.child(label),
            )
            .child(
                div()
                    .min_w_0()
                    .flex_1()
                    .flex()
                    .items_center()
                    .gap_1()
                    .when(!bound, |this| {
                        this.child(Self::choice_dropdown(
                            &common.target,
                            current,
                            options,
                            common.read_only,
                            &ctx.inspector,
                        ))
                    })
                    .when_some(binding_button, |this, button| this.child(button)),
            )
            .into_any_element();
        (row, bound)
    }

    fn color_full_row(
        common: &LeafControl,
        picker: &Entity<ColorPickerState>,
        animation_enabled: bool,
        binding: Option<SceneFieldBinding>,
        ctx: &RenderCtx,
    ) -> gpui::AnyElement {
        let is_bound = binding
            .as_ref()
            .is_some_and(|binding| binding.connected.is_some());
        let binding_button = binding.map(|binding| {
            Self::scene_binding_button(
                binding,
                &ctx.inspector,
                SharedString::from(format!("bind-scene-argument-{}", common.target.key)),
            )
        });
        let animation_button = (common.animatable && !is_bound).then(|| {
            Self::color_animation_toggle(&common.target, animation_enabled, &ctx.inspector)
        });
        let value = Self::color_editor(common, picker, ctx);
        let select_inspector = ctx.inspector.clone();
        let select_target = common.target.clone();
        div()
            .w_full()
            .flex()
            .items_center()
            .gap_3()
            .child(Self::property_label_column(common.label.clone()))
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
                                .when(!common.read_only, |this| {
                                    this.on_mouse_down(MouseButton::Left, move |_, _, cx| {
                                        select_inspector.update(cx, |inspector, cx| {
                                            inspector.select_animation(&select_target, cx);
                                        });
                                    })
                                })
                                .child(value),
                        )
                    })
                    .when_some(animation_button, |this, button| this.child(button))
                    .when_some(binding_button, |this, button| this.child(button)),
            )
            .into_any_element()
    }

    fn color_compact_row(
        common: &LeafControl,
        picker: &Entity<ColorPickerState>,
        animation_enabled: bool,
        binding: Option<SceneFieldBinding>,
        ctx: &RenderCtx,
    ) -> (gpui::AnyElement, bool) {
        let is_bound = binding
            .as_ref()
            .is_some_and(|binding| binding.connected.is_some());
        let binding_button = binding.map(|binding| {
            Self::scene_binding_button(
                binding,
                &ctx.inspector,
                SharedString::from(format!("bind-scene-argument-{}", common.target.key)),
            )
        });
        let animation_button = (common.animatable && !is_bound).then(|| {
            Self::color_animation_toggle(&common.target, animation_enabled, &ctx.inspector)
        });
        let value = Self::color_editor(common, picker, ctx);
        let select_inspector = ctx.inspector.clone();
        let select_target = common.target.clone();
        let row = div()
            .min_w_0()
            .w_full()
            .flex()
            .items_center()
            .gap_2()
            .when_some(
                Self::scalar_label(common.scalar_label.clone()),
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
                                .when(!common.read_only, |this| {
                                    this.on_mouse_down(MouseButton::Left, move |_, _, cx| {
                                        select_inspector.update(cx, |inspector, cx| {
                                            inspector.select_animation(&select_target, cx);
                                        });
                                    })
                                })
                                .child(value),
                        )
                    })
                    .when_some(animation_button, |this, button| this.child(button))
                    .when_some(binding_button, |this, button| this.child(button)),
            )
            .into_any_element();
        (row, is_bound)
    }

    /// Full labeled row for a top-level scalar: item/effect properties,
    /// scalar rows, and scene argument values.
    pub(super) fn scalar_full_row(control: &Control, ctx: &RenderCtx) -> Option<gpui::AnyElement> {
        match control {
            Control::Number(number) => {
                let common = &number.common;
                let input = ctx.store.text(&common.id)?;
                Some(
                    Self::number_full_row(
                        common,
                        &number.spec,
                        &input,
                        common.animation_enabled,
                        common.binding.clone(),
                        ctx,
                    )
                    .into_any_element(),
                )
            }
            Control::Text(text) => {
                let common = &text.common;
                let input = ctx.store.text(&common.id)?;
                Some(
                    Self::text_full_row(
                        common,
                        text.multiline,
                        &input,
                        common.binding.clone(),
                        ctx,
                    )
                    .into_any_element(),
                )
            }
            Control::Bool(boolean) => {
                let common = &boolean.common;
                let PropertyValue::Bool(value) = common.value else {
                    return None;
                };
                Some(Self::toggle_full_row(
                    common,
                    value,
                    common.mixed,
                    common.binding.clone(),
                    ctx,
                ))
            }
            Control::Choice(choice) => {
                let common = &choice.common;
                let PropertyValue::Enum(current) = common.value else {
                    return None;
                };
                Some(Self::dropdown_full_row(
                    common,
                    current,
                    &choice.options,
                    common.binding.clone(),
                    ctx,
                ))
            }
            Control::Color(color) => {
                let common = &color.common;
                let picker = ctx.store.color(&common.id)?;
                Some(Self::color_full_row(
                    common,
                    &picker,
                    common.animation_enabled,
                    common.binding.clone(),
                    ctx,
                ))
            }
            Control::Group { .. } => None,
        }
    }

    /// Compact scalar row for tuple children and array elements.
    /// Returns each row with whether its scene argument is connected.
    pub(super) fn scalar_compact_row(
        control: &Control,
        ctx: &RenderCtx,
        size_locked: bool,
    ) -> Option<(gpui::AnyElement, bool)> {
        match control {
            Control::Number(number) => {
                let common = &number.common;
                let input = ctx.store.text(&common.id)?;
                Some(Self::number_compact_row(
                    common,
                    &number.spec,
                    &input,
                    size_locked,
                    ctx,
                ))
            }
            Control::Text(text) => {
                let common = &text.common;
                let input = ctx.store.text(&common.id)?;
                Some(Self::text_compact_row(
                    common,
                    text.multiline,
                    &input,
                    common.binding.clone(),
                    ctx,
                ))
            }
            Control::Bool(boolean) => {
                let common = &boolean.common;
                let PropertyValue::Bool(value) = common.value else {
                    return None;
                };
                Some(Self::toggle_compact_row(
                    common,
                    value,
                    common.mixed,
                    common.binding.clone(),
                    ctx,
                ))
            }
            Control::Choice(choice) => {
                let common = &choice.common;
                let PropertyValue::Enum(current) = common.value else {
                    return None;
                };
                Some(Self::dropdown_compact_row(
                    common,
                    current,
                    &choice.options,
                    common.binding.clone(),
                    ctx,
                ))
            }
            Control::Color(color) => {
                let common = &color.common;
                let picker = ctx.store.color(&common.id)?;
                Some(Self::color_compact_row(
                    common,
                    &picker,
                    common.animation_enabled,
                    common.binding.clone(),
                    ctx,
                ))
            }
            Control::Group { .. } => None,
        }
    }

    /// Grouped tuple rendering: one property label with per-scalar rows.
    /// A lone child renders as a plain full row, matching single scalars.
    /// The resolver marks the item-level size tuple with `size_key`; other
    /// groups cannot accidentally inherit the aspect-ratio constraint.
    pub(super) fn group_box(
        label: String,
        children: &[Control],
        size_key: Option<InspectorPath>,
        aspect: Option<AspectRatioLockState>,
        ctx: &RenderCtx,
    ) -> gpui::AnyElement {
        if let [child] = children
            && let Some(row) = Self::scalar_full_row(child, ctx)
        {
            return row;
        }
        let is_size_group = size_key.is_some();
        let aspect_row = aspect.zip(size_key).map(|(state, key)| {
            div()
                .w_full()
                .flex()
                .items_center()
                .justify_end()
                .gap_2()
                .child(Self::aspect_ratio_control(
                    key,
                    state,
                    ctx.colors.muted_foreground,
                    ctx.editor,
                ))
        });
        let size_locked = is_size_group && aspect.is_some_and(|state| state.checked());
        let rows = children
            .iter()
            .filter_map(|child| {
                Self::scalar_compact_row(child, ctx, size_locked).map(|(row, _)| row)
            })
            .collect::<Vec<_>>();
        div()
            .w_full()
            .flex()
            .items_start()
            .gap_3()
            .child(Self::property_label_column(label))
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

    pub(super) fn elements_section(
        group: &ElementGroup,
        children: &[Control],
        ctx: &RenderCtx,
        separator_color: gpui::Hsla,
        allow_structure_edit: bool,
    ) -> gpui::AnyElement {
        let item_id = ctx.item_id;
        let owner = SceneBindingOwner::from_effect(group.target.effect_id);
        let property_label = group.property.label().to_owned();
        let rows_are_tuples = matches!(
            group.property.ty(),
            PropertyType::Array {
                element_type: PropertyValueType::Tuple(_),
                ..
            }
        );
        let rows_have_scene_binding = group.has_scene_binding;
        let mut rows = div().w_full().min_w_0().flex().flex_col().gap_1();
        for (element_index, row) in group.elements.iter().enumerate() {
            let row_controls = match children.get(element_index) {
                Some(Control::Group { children, .. }) => children.as_slice(),
                _ => &[],
            };
            let scene_binding = (group.element_kind == ElementKind::FontFamily)
                .then(|| {
                    row_controls.iter().find_map(|control| {
                        control.common().and_then(|common| common.binding.clone())
                    })
                })
                .flatten();
            let is_scene_bound = scene_binding
                .as_ref()
                .is_some_and(|binding| binding.connected.is_some());
            let binding_button = scene_binding.map(|binding| {
                Self::scene_binding_button(
                    binding,
                    &ctx.inspector,
                    SharedString::from(format!(
                        "bind-scene-array-{}-{element_index}",
                        group.target.property_id
                    )),
                )
            });
            let mut row_has_binding = is_scene_bound;
            let mut value_rows = div()
                .min_w_0()
                .flex()
                .flex_col()
                .gap_1()
                .when(rows_are_tuples, |this| this.w_full())
                .when(!rows_are_tuples, |this| this.flex_1());
            if group.element_kind != ElementKind::FontFamily {
                for control in row_controls {
                    let row = if rows_are_tuples {
                        Self::scalar_compact_row(control, ctx, false)
                    } else {
                        Self::element_scalar(control, group, element_index, owner, ctx)
                    };
                    if let Some((row, bound)) = row {
                        row_has_binding |= bound;
                        value_rows = value_rows.child(row);
                    }
                }
            }

            match group.element_kind {
                ElementKind::Scalar => {}
                ElementKind::FontFamily => {
                    let PropertyValue::String(selected_font) = row.value() else {
                        continue;
                    };
                    let font_choices = ctx
                        .font_names
                        .iter()
                        .filter(|font| {
                            !group.elements.iter().enumerate().any(|(index, value)| {
                                index != element_index
                                    && matches!(value.value(), PropertyValue::String(selected) if selected == *font)
                            })
                        })
                        .map(|font| SearchPickerEntry::new(font.clone(), "", font.clone()))
                        .collect::<Vec<_>>();
                    let picker_inspector = ctx.inspector.clone();
                    let mut picker_target = group.target.clone();
                    picker_target.path = InspectorPath::new(Some(row.element_id()), None);
                    let label = if selected_font.is_empty() {
                        "フォントを選択".to_owned()
                    } else {
                        selected_font.clone()
                    };
                    let mut trigger = Button::new(SharedString::from(format!(
                        "{}-array-{element_index}-font",
                        group.target.key
                    )))
                    .small()
                    .w_full()
                    .label(label)
                    .dropdown_caret(true);
                    let trigger_style = trigger.style().clone();
                    value_rows = value_rows.child(
                        Popover::new(SharedString::from(format!(
                            "{}-array-{element_index}-font-picker",
                            group.target.key
                        )))
                        .trigger_style(trigger_style)
                        .trigger(trigger)
                        .content(move |window, cx| {
                            let inspector = picker_inspector.clone();
                            let target = picker_target.clone();
                            let entries = font_choices.clone();
                            cx.new(|cx| {
                                SearchPicker::new(
                                    entries,
                                    "フォントを検索",
                                    move |font, _, cx| {
                                        inspector.update(cx, |inspector, cx| {
                                            if inspector
                                                .editor
                                                .read(cx)
                                                .selected_item()
                                                .is_some_and(|item| item.id == item_id)
                                                && inspector.set_scalar(
                                                    &target,
                                                    PropertyValue::String(font),
                                                    cx,
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

            let move_up_editor = ctx.editor.clone();
            let move_up_property_id = group.target.property_id.clone();
            let move_up_effect_id = group.target.effect_id;
            let mut moved_up = group.elements.clone();
            if element_index > 0 {
                moved_up.swap(element_index, element_index - 1);
            }
            let move_up_button = Button::new(SharedString::from(format!(
                "array-{}-{}-{element_index}-up",
                item_id.get(),
                group.target.property_id
            )))
            .small()
            .compact()
            .ghost()
            .icon(IconName::ChevronUp)
            .tooltip("上へ移動")
            .disabled(element_index == 0 || rows_have_scene_binding)
            .on_click(move |_, _, cx| {
                move_up_editor.update(cx, |editor, cx| {
                    if editor
                        .selected_item()
                        .is_some_and(|item| item.id == item_id)
                        && Self::update_elements(
                            editor,
                            move_up_effect_id,
                            &move_up_property_id,
                            PropertyValue::Array(moved_up.clone()),
                        )
                    {
                        cx.notify();
                    }
                });
            });

            let move_down_editor = ctx.editor.clone();
            let move_down_property_id = group.target.property_id.clone();
            let move_down_effect_id = group.target.effect_id;
            let mut moved_down = group.elements.clone();
            if element_index + 1 < moved_down.len() {
                moved_down.swap(element_index, element_index + 1);
            }
            let move_down_button = Button::new(SharedString::from(format!(
                "array-{}-{}-{element_index}-down",
                item_id.get(),
                group.target.property_id
            )))
            .small()
            .compact()
            .ghost()
            .icon(IconName::ChevronDown)
            .tooltip("下へ移動")
            .disabled(element_index + 1 == group.elements.len() || rows_have_scene_binding)
            .on_click(move |_, _, cx| {
                move_down_editor.update(cx, |editor, cx| {
                    if editor
                        .selected_item()
                        .is_some_and(|item| item.id == item_id)
                        && Self::update_elements(
                            editor,
                            move_down_effect_id,
                            &move_down_property_id,
                            PropertyValue::Array(moved_down.clone()),
                        )
                    {
                        cx.notify();
                    }
                });
            });

            let remove_editor = ctx.editor.clone();
            let remove_property_id = group.target.property_id.clone();
            let remove_effect_id = group.target.effect_id;
            let mut remaining = group.elements.clone();
            remaining.remove(element_index);
            let remove_button = Button::new(SharedString::from(format!(
                "array-{}-{}-{element_index}-remove",
                item_id.get(),
                group.target.property_id
            )))
            .small()
            .compact()
            .ghost()
            .icon(IconName::Delete)
            .tooltip("削除")
            .disabled(
                group.elements.len() <= group.min_items as usize
                    || row_has_binding
                    || rows_have_scene_binding,
            )
            .on_click(move |_, _, cx| {
                remove_editor.update(cx, |editor, cx| {
                    if editor
                        .selected_item()
                        .is_some_and(|item| item.id == item_id)
                        && Self::update_elements(
                            editor,
                            remove_effect_id,
                            &remove_property_id,
                            PropertyValue::Array(remaining.clone()),
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
            let row = if rows_are_tuples {
                div()
                    .w_full()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .pb_2()
                    .when(element_index > 0, |this| {
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
                                    .child(format!("要素 {}", element_index + 1)),
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

        let add_disabled =
            group.elements.len() >= group.max_items as usize || rows_have_scene_binding;
        let add_editor = ctx.editor.clone();
        let add_property_id = group.target.property_id.clone();
        let add_effect_id = group.target.effect_id;
        let next_value = model::append_default(group);
        let add_control = Button::new(SharedString::from(format!(
            "array-{}-{}-add",
            item_id.get(),
            group.target.property_id
        )))
        .small()
        .w_full()
        .label(format!("{}を追加", group.property.label()))
        .disabled(add_disabled)
        .on_click(move |_, _, cx| {
            add_editor.update(cx, |editor, cx| {
                if editor
                    .selected_item()
                    .is_some_and(|item| item.id == item_id)
                    && Self::push_element(
                        editor,
                        add_effect_id,
                        &add_property_id,
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
            .child(Self::property_label_column(property_label))
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

    /// Array element scalar rendering. Tuple components go through the
    /// shared compact rows; plain scalars keep the array element layout.
    /// Returns each row with whether its scene argument is connected.
    fn element_scalar(
        control: &Control,
        group: &ElementGroup,
        element_index: usize,
        owner: SceneBindingOwner,
        ctx: &RenderCtx,
    ) -> Option<(gpui::AnyElement, bool)> {
        match control {
            Control::Number(number) if number.common.target.path.scalar_index().is_some() => {
                let input = ctx.store.text(&number.common.id)?;
                Some(Self::number_compact_row(
                    &number.common,
                    &number.spec,
                    &input,
                    false,
                    ctx,
                ))
            }
            Control::Number(number) => {
                Self::element_number(number, group, element_index, owner, ctx)
            }
            Control::Color(color) => {
                let picker = ctx.store.color(&color.common.id)?;
                let bound = color
                    .common
                    .binding
                    .as_ref()
                    .is_some_and(|binding| binding.connected.is_some());
                let row = Self::color_full_row(
                    &color.common,
                    &picker,
                    color.common.animation_enabled,
                    color.common.binding.clone(),
                    ctx,
                );
                Some((row, bound))
            }
            Control::Text(_) | Control::Bool(_) | Control::Choice(_) => {
                let row = Self::scalar_full_row(control, ctx)?;
                let bound = control
                    .common()
                    .and_then(|common| common.binding.as_ref())
                    .is_some_and(|binding| binding.connected.is_some());
                Some((row, bound))
            }
            Control::Group { .. } => None,
        }
    }

    fn element_number(
        number: &NumberControl,
        group: &ElementGroup,
        element_index: usize,
        owner: SceneBindingOwner,
        ctx: &RenderCtx,
    ) -> Option<(gpui::AnyElement, bool)> {
        let _ = owner;
        let common = &number.common;
        let spec = &number.spec;
        let component = common.target.path.scalar_index();
        let input = ctx.store.text(&common.id)?;
        let animation_enabled = common.animation_enabled;
        let binding = common.binding.clone();
        let component_is_bound = binding
            .as_ref()
            .is_some_and(|binding| binding.connected.is_some());
        let component_binding_button = binding.map(|binding| {
            Self::scene_binding_button(
                binding,
                &ctx.inspector,
                SharedString::from(format!(
                    "bind-scene-array-{}-{element_index}-{component:?}",
                    group.target.property_id
                )),
            )
        });
        let value_input = Self::number_editor(common, spec, &input, false, ctx);
        let animation_button = (common.animatable && !component_is_bound).then(|| {
            Self::number_animation_toggle(
                &common.target,
                spec,
                animation_enabled,
                if animation_enabled {
                    "この座標のアニメーションを解除".to_owned()
                } else {
                    "要素全体をアニメーションする".to_owned()
                },
                &ctx.inspector,
            )
        });
        let select_inspector = ctx.inspector.clone();
        let select_target = common.target.clone();
        let select_spec = spec.clone();
        let row = div()
            .min_w_0()
            .w_full()
            .flex()
            .items_center()
            .gap_2()
            .when(!component_is_bound, |this| {
                this.child(
                    div()
                        .id(SharedString::from(common.target.key.to_string()))
                        .min_w_0()
                        .flex()
                        .flex_1()
                        .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                            select_inspector.update(cx, |inspector, cx| {
                                inspector.select_number_animation(&select_target, &select_spec, cx);
                            });
                        })
                        .child(value_input),
                )
            })
            .when_some(animation_button, |this, button| this.child(button))
            .when_some(component_binding_button, |this, button| this.child(button))
            .into_any_element();
        Some((row, component_is_bound))
    }
}
