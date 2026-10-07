use rust_i18n::t;

use super::control::{
    Control, EditorControl, ElementGroup, ElementKind, LeafControl, NumberControl,
};
use super::edit::ArrayEdit;
use super::state::ControlStore;
use super::*;

mod editors;
mod layout;

pub(super) struct RenderCtx<'a> {
    pub colors: ThemeColor,
    pub editor: &'a Entity<TimelineEditor>,
    pub animation_address: Option<PropertyAddress>,
    pub inspector: Entity<PropertyInspector>,
    pub store: &'a ControlStore,
    pub font_names: &'a [String],
    pub item_id: ItemId,
    pub scope: EditScope,
    pub multiple: bool,
    pub selecting_file: bool,
    pub file_input: Entity<FileInputController>,
    pub scene_overrides: std::collections::HashSet<String>,
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

    fn animation_address_is_focused(common: &LeafControl, ctx: &RenderCtx) -> bool {
        ctx.animation_address.as_ref() == Some(&common.target.address(ctx.item_id))
    }

    fn focused_animation_label(
        label: impl Into<SharedString>,
        common: &LeafControl,
        ctx: &RenderCtx,
        width: AnimationLabelWidth,
        id_prefix: &str,
    ) -> gpui::AnyElement {
        let target = common.target.clone();
        let focused = Self::animation_address_is_focused(common, ctx);
        let inspector = ctx.inspector.clone();
        Self::animation_label_base(label.into(), width, focused, ctx)
            .id(SharedString::from(format!("{id_prefix}-{:?}", common.id)))
            .when(
                common.animation_enabled && common.animation_stops.is_empty() && ctx.multiple,
                |this| {
                    this.tooltip(|window, cx| {
                        ::ui::tooltip::Tooltip::new(
                            t!("inspector.animation_individual_edit").to_string(),
                        )
                        .build(window, cx)
                    })
                },
            )
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
        let text = div()
            .w_full()
            .min_w_0()
            .text_sm()
            .when(matches!(width, AnimationLabelWidth::Fixed(_)), |this| {
                this.whitespace_normal()
            })
            .when(matches!(width, AnimationLabelWidth::Fill), |this| {
                this.overflow_hidden().whitespace_nowrap().text_ellipsis()
            })
            .when(focused, |this| this.text_color(ctx.colors.primary))
            .child(label);
        let label = div()
            .min_h(px(24.))
            .min_w_0()
            .flex()
            .items_center()
            .child(text);
        match width {
            AnimationLabelWidth::Fixed(width) => label.w(px(width)).flex_none(),
            AnimationLabelWidth::Fill => label.w_0().flex_1(),
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
                AnimationLabelWidth::Fixed(Self::SCALAR_LABEL_WIDTH),
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
            .is_some_and(|common| Self::animation_address_is_focused(common, ctx))
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
        Button::new(key)
            .small()
            .compact()
            .ghost()
            .icon(IconName::Link)
            .when_some(binding.connected.as_ref(), |button, (_, label)| {
                button.label(label.clone())
            })
            .tooltip(t!("rows.bind_tooltip").to_string())
            .popup_menu(move |menu, _, _| {
                if let Some((argument_id, _)) = &binding.connected {
                    let settings_inspector = menu_inspector.clone();
                    let settings_id = argument_id.clone();
                    let inspector = menu_inspector.clone();
                    let argument_id = argument_id.clone();
                    let target = binding.target.clone();
                    return menu
                        .item(
                            PopupMenuItem::new(t!("rows.open_argument_settings").to_string())
                                .on_click(move |_, _, cx| {
                                    settings_inspector.update(cx, |inspector, cx| {
                                        inspector.request_scene_argument_settings(&settings_id, cx)
                                    });
                                }),
                        )
                        .item(PopupMenuItem::new(t!("rows.unbind").to_string()).on_click(
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
                                            notifications
                                                .push(t!("rows.unbind_failed").to_string(), cx);
                                        });
                                    }
                                });
                            },
                        ));
                }

                binding
                    .compatible
                    .iter()
                    .fold(menu, |menu, (argument_id, label)| {
                        let inspector = menu_inspector.clone();
                        let argument_id = argument_id.clone();
                        let target = binding.target.clone();
                        menu.item(PopupMenuItem::new(label.clone()).on_click(move |_, _, cx| {
                            inspector.update(cx, |inspector, cx| {
                                inspector.bind_scene_argument(&argument_id, target.clone(), cx);
                            });
                        }))
                    })
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
        enabled: bool,
        tooltip: String,
        inspector: &Entity<Self>,
    ) -> Button {
        let inspector = inspector.clone();
        let target = target.clone();
        Self::keyframe_base(
            SharedString::from(format!("toggle-animation-{}", target.key)),
            enabled,
            tooltip,
        )
        .on_click(move |_, window, cx| {
            inspector.update(cx, |inspector, cx| {
                inspector.set_animation_enabled(&target, !enabled, window, cx);
            });
        })
    }

    fn coordinate_animation_toggle(
        target: &PropertyTarget,
        enabled: bool,
        disabled: bool,
        visible: bool,
        inspector: &Entity<Self>,
    ) -> Button {
        let inspector = inspector.clone();
        let target = target.clone();
        Self::keyframe_base(
            SharedString::from(format!("toggle-animation-{}", target.key)),
            visible,
            if disabled {
                t!("rows.aspect_auto").to_string()
            } else if enabled {
                t!("rows.unanimate_axis").to_string()
            } else {
                t!("rows.animate_axis").to_string()
            },
        )
        .tab_stop(!disabled)
        .when(!disabled, |button| {
            button.on_click(move |_, window, cx| {
                cx.stop_propagation();
                inspector.update(cx, |inspector, cx| {
                    inspector.set_animation_enabled(&target, !enabled, window, cx);
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
                t!("rows.unanimate").to_string()
            } else {
                t!("rows.animate_color").to_string()
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
            ArrayEdit::MoveUp(_) => ("up", IconName::ChevronUp, t!("rows.move_up").to_string()),
            ArrayEdit::MoveDown(_) => (
                "down",
                IconName::ChevronDown,
                t!("rows.move_down").to_string(),
            ),
            ArrayEdit::Remove(_) => ("remove", IconName::Delete, t!("rows.remove").to_string()),
        };
        let editor = ctx.editor.clone();
        let address = group.target.address(item_id);
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
                if Self::edit_array(editor, &address, edit) {
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
                        Self::scalar_compact_row(control, ctx)
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
                    picker_target.element_id = Some(row.element_id());
                    picker_target.scalar_index = None;
                    let label = if selected_font.is_empty() {
                        t!("rows.choose_font").to_string()
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
                                    t!("rows.search_font").to_string(),
                                    move |font, _, cx| {
                                        inspector.update(cx, |inspector, cx| {
                                            if inspector
                                                .inspector_item_at_playhead(cx)
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
                                t!("rows.element", index = element_index + 1).to_string(),
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
        let add_address = group.target.address(item_id);
        let next_value = group
            .property
            .append_default_value()
            .expect("array properties declare append_default")
            .clone();
        let add_control = Button::new(SharedString::from(format!(
            "array-{}-{}-add",
            item_id.get(),
            group.target.property_id
        )))
        .small()
        .w_full()
        .label(t!("rows.add_element", label = group.property.label()).to_string())
        .disabled(add_disabled)
        .on_click(move |_, _, cx| {
            add_editor.update(cx, |editor, cx| {
                if Self::push_element(editor, &add_address, next_value.clone()) {
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
            Control::Number(number) if number.common.target.scalar_index.is_some() => {
                let input = ctx.store.text(&number.common.id)?;
                Some(Self::number_compact_row(
                    &number.common,
                    &number.spec,
                    &input,
                    ctx,
                ))
            }
            Control::Number(number) => Self::element_number(number, group, element_index, ctx),
            Control::Color(color) => {
                let picker = ctx.store.color(&color.id)?;
                let bound = color
                    .binding
                    .as_ref()
                    .is_some_and(|binding| binding.connected.is_some());
                let row = Self::color_full_row(
                    color,
                    &picker,
                    color.animation_enabled,
                    color.binding.clone(),
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
            Control::File(_) => Self::scalar_compact_row(control, ctx),
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
        let component = common.target.scalar_index;
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
        let value_input = Self::number_editor(common, spec, &input, common.read_only, ctx);
        let animation_button = (common.animatable && !component_is_bound).then(|| {
            Self::number_animation_toggle(
                &common.target,
                animation_enabled,
                if animation_enabled {
                    t!("rows.unanimate_axis").to_string()
                } else {
                    t!("rows.animate_element").to_string()
                },
                &ctx.inspector,
            )
        });
        let select_inspector = ctx.inspector.clone();
        let select_target = common.target.clone();
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
                                inspector.select_animation(&select_target, cx);
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
