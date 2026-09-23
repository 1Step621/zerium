use super::control::{Control, ElementGroup, ElementKind, LeafControl, NumberControl};
use super::edit::ArrayEdit;
use super::state::ControlStore;
use super::*;

mod editors;
mod layout;

pub(super) struct RenderCtx<'a> {
    pub colors: ThemeColor,
    pub editor: &'a Entity<TimelineEditor>,
    pub animation_target: Option<AnimationTarget>,
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

#[derive(Clone, Copy)]
enum AnimationLabelWidth {
    Fixed(f32),
    Fill,
}

impl PropertyInspector {
    // Layout primitives. Editors below only build their value widget and
    // these helpers provide the shared labeled/compact row geometry.
    fn labeled_row(label: gpui::AnyElement, content: Div) -> Div {
        div()
            .w_full()
            .flex()
            .items_center()
            .gap_3()
            .child(label)
            .child(content)
    }

    fn compact_row(label: Option<gpui::AnyElement>, content: Div) -> Div {
        div()
            .min_w_0()
            .w_full()
            .flex()
            .items_center()
            .gap_2()
            .when_some(label, |this, label| this.child(label))
            .child(content)
    }

    fn animation_target_is_focused(common: &LeafControl, ctx: &RenderCtx) -> bool {
        ctx.animation_target
            .as_ref()
            .is_some_and(|target| common.target.matches_animation_target(ctx.item_id, target))
    }

    fn focused_animation_label(
        label: impl Into<SharedString>,
        common: &LeafControl,
        ctx: &RenderCtx,
        width: AnimationLabelWidth,
        id_prefix: &str,
    ) -> gpui::AnyElement {
        let target = common.target.clone();
        let focused = Self::animation_target_is_focused(common, ctx);
        let inspector = ctx.inspector.clone();
        Self::animation_label_base(label.into(), width, focused, ctx)
            .id(SharedString::from(format!("{id_prefix}-{:?}", common.id)))
            .when(common.animation_enabled, |this| {
                this.cursor_pointer().on_click(move |_, _, cx| {
                    inspector.update(cx, |inspector, cx| {
                        inspector.select_animation(&target, cx);
                    });
                })
            })
            .into_any_element()
    }

    fn animation_label_base(
        label: SharedString,
        width: AnimationLabelWidth,
        focused: bool,
        ctx: &RenderCtx,
    ) -> Div {
        let label = div()
            .h(px(24.))
            .flex()
            .items_center()
            .text_sm()
            .when(focused, |this| this.text_color(ctx.colors.primary))
            .child(label);
        match width {
            AnimationLabelWidth::Fixed(width) => label.w(px(width)).flex_none(),
            AnimationLabelWidth::Fill => label.flex_1(),
        }
    }

    fn animation_property_label(
        label: impl Into<SharedString>,
        common: &LeafControl,
        ctx: &RenderCtx,
    ) -> gpui::AnyElement {
        Self::focused_animation_label(
            label,
            common,
            ctx,
            AnimationLabelWidth::Fixed(Self::PROPERTY_LABEL_WIDTH),
            "property-label",
        )
    }

    fn animation_scalar_label(common: &LeafControl, ctx: &RenderCtx) -> Option<gpui::AnyElement> {
        common.scalar_label.clone().map(|label| {
            Self::focused_animation_label(
                label,
                common,
                ctx,
                AnimationLabelWidth::Fixed(32.),
                "scalar-label",
            )
        })
    }

    fn animation_container_label(
        label: impl Into<SharedString>,
        children: &[Control],
        ctx: &RenderCtx,
        width: AnimationLabelWidth,
        id_prefix: &str,
    ) -> gpui::AnyElement {
        if let [child] = children
            && let Some(common) = child.common()
        {
            return Self::focused_animation_label(label, common, ctx, width, id_prefix);
        }
        let focused = children
            .iter()
            .any(|child| Self::control_contains_focused_animation(child, ctx));
        Self::animation_label_base(label.into(), width, focused, ctx).into_any_element()
    }

    fn control_contains_focused_animation(control: &Control, ctx: &RenderCtx) -> bool {
        control
            .common()
            .is_some_and(|common| Self::animation_target_is_focused(common, ctx))
            || matches!(control, Control::Group { children, .. } if children
            .iter()
            .any(|child| Self::control_contains_focused_animation(child, ctx)))
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

    fn array_edit_button(
        group: &ElementGroup,
        item_id: ItemId,
        element_index: usize,
        disabled: bool,
        edit: ArrayEdit,
        ctx: &RenderCtx,
    ) -> Button {
        let (suffix, icon, tooltip) = match edit {
            ArrayEdit::MoveUp(_) => ("up", IconName::ChevronUp, "上へ移動"),
            ArrayEdit::MoveDown(_) => ("down", IconName::ChevronDown, "下へ移動"),
            ArrayEdit::Remove(_) => ("remove", IconName::Delete, "削除"),
        };
        let editor = ctx.editor.clone();
        let effect_id = group.target.effect_id;
        let property_id = group.target.property_id.clone();
        Button::new(SharedString::from(format!(
            "array-{}-{}-{element_index}-{suffix}",
            item_id.get(),
            property_id
        )))
        .small()
        .compact()
        .ghost()
        .icon(icon)
        .tooltip(tooltip)
        .disabled(disabled)
        .on_click(move |_, _, cx| {
            editor.update(cx, |editor, cx| {
                if editor
                    .selected_item()
                    .is_some_and(|item| item.id == item_id)
                    && Self::edit_selected_array(editor, effect_id, &property_id, edit)
                {
                    cx.notify();
                }
            });
        })
    }

    pub(super) fn elements_section(
        group: &ElementGroup,
        children: &[Control],
        ctx: &RenderCtx,
        separator_color: gpui::Hsla,
        allow_structure_edit: bool,
    ) -> gpui::AnyElement {
        let item_id = ctx.item_id;
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
                        Self::element_scalar(control, group, element_index, ctx)
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

            let element_id = row.element_id();
            let move_up_button = Self::array_edit_button(
                group,
                item_id,
                element_index,
                element_index == 0 || rows_have_scene_binding,
                ArrayEdit::MoveUp(element_id),
                ctx,
            );
            let move_down_button = Self::array_edit_button(
                group,
                item_id,
                element_index,
                element_index + 1 == group.elements.len() || rows_have_scene_binding,
                ArrayEdit::MoveDown(element_id),
                ctx,
            );
            let remove_button = Self::array_edit_button(
                group,
                item_id,
                element_index,
                group.elements.len() <= group.min_items as usize
                    || row_has_binding
                    || rows_have_scene_binding,
                ArrayEdit::Remove(element_id),
                ctx,
            );
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
                            .child(Self::animation_container_label(
                                format!("要素 {}", element_index + 1),
                                row_controls,
                                ctx,
                                AnimationLabelWidth::Fill,
                                "element-label",
                            ))
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
            .child(Self::animation_container_label(
                property_label,
                children,
                ctx,
                AnimationLabelWidth::Fixed(Self::PROPERTY_LABEL_WIDTH),
                "group-label",
            ))
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
            Control::Number(number) => Self::element_number(number, group, element_index, ctx),
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
        ctx: &RenderCtx,
    ) -> Option<(gpui::AnyElement, bool)> {
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
