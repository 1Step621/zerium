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
    pub selecting_file: bool,
    pub file_input: Entity<FileInputController>,
    pub scene_overrides: std::collections::HashSet<String>,
}

#[derive(Clone, Copy)]
enum AnimationLabelWidth {
    Content,
    Fill,
}

pub(super) struct PropertyRow {
    label: Option<gpui::AnyElement>,
    content: Div,
}

impl PropertyRow {
    fn new(label: impl Into<Option<gpui::AnyElement>>, content: Div) -> Self {
        Self {
            label: label.into(),
            content,
        }
    }

    pub(super) fn into_cells(self) -> Vec<gpui::AnyElement> {
        let content = self.content.min_w_0().w_full();
        match self.label {
            Some(label) => vec![label, content.into_any_element()],
            None => vec![content.col_span_full().into_any_element()],
        }
    }
}

impl IntoElement for PropertyRow {
    type Element = Div;

    fn into_element(self) -> Div {
        PropertyInspector::property_grid().children(self.into_cells())
    }
}

impl PropertyInspector {
    pub(super) fn property_grid() -> Div {
        div()
            .grid()
            .grid_template_columns([
                gpui::GridTrackSize::FitContent(gpui::relative(0.3)),
                gpui::GridTrackSize::Fraction(1.),
            ])
            .min_w_0()
            .w_full()
            .items_center()
            .gap_x_3()
            .gap_y_1()
    }

    fn animation_address_is_focused(common: &LeafControl, ctx: &RenderCtx) -> bool {
        ctx.animation_address.as_ref() == Some(&common.target)
    }

    fn focused_animation_label(
        label: impl Into<SharedString>,
        common: &LeafControl,
        ctx: &RenderCtx,
        width: AnimationLabelWidth,
        id: SharedString,
    ) -> gpui::AnyElement {
        let target = common.target.clone();
        let focused = Self::animation_address_is_focused(common, ctx);
        let inspector = ctx.inspector.clone();
        Self::animation_label_base(label.into(), width, focused, ctx)
            .id(id)
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
        let tooltip = label.clone();
        let text = div()
            .min_w_0()
            .flex_1()
            .text_sm()
            .overflow_hidden()
            .whitespace_nowrap()
            .text_ellipsis()
            .when(focused, |this| this.text_color(ctx.colors.primary))
            .child(label.replace(['\n', '\r'], " "));
        let label = div()
            .min_h(px(24.))
            .min_w_0()
            .flex()
            .items_center()
            .tooltip(move |window, cx| {
                ::ui::tooltip::Tooltip::new(tooltip.clone()).build(window, cx)
            })
            .child(text);
        match width {
            AnimationLabelWidth::Content => label.max_w(px(90.)),
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
            AnimationLabelWidth::Content,
            format!("property-label-{:?}", common.id).into(),
        )
    }

    fn animation_scalar_label(common: &LeafControl, ctx: &RenderCtx) -> Option<gpui::AnyElement> {
        common.scalar_label.clone().map(|label| {
            Self::focused_animation_label(
                label,
                common,
                ctx,
                AnimationLabelWidth::Content,
                format!("scalar-label-{:?}", common.id).into(),
            )
        })
    }

    fn animation_container_label(
        label: impl Into<SharedString>,
        children: &[Control],
        ctx: &RenderCtx,
        width: AnimationLabelWidth,
        id: SharedString,
    ) -> gpui::AnyElement {
        if let [child] = children
            && let Some(common) = child.common()
        {
            return Self::focused_animation_label(label, common, ctx, width, id);
        }
        let focused = children
            .iter()
            .any(|child| Self::control_contains_focused_animation(child, ctx));
        Self::animation_label_base(label.into(), width, focused, ctx)
            .id(id)
            .into_any_element()
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
        target: &PropertyAddress,
        enabled: bool,
        tooltip: String,
        inspector: &Entity<Self>,
    ) -> Button {
        let inspector = inspector.clone();
        let target = target.clone();
        Self::keyframe_base(
            SharedString::from(format!("toggle-animation-{:?}", target)),
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
        target: &PropertyAddress,
        enabled: bool,
        disabled: bool,
        visible: bool,
        inspector: &Entity<Self>,
    ) -> Button {
        let inspector = inspector.clone();
        let target = target.clone();
        Self::keyframe_base(
            SharedString::from(format!("toggle-animation-{:?}", target)),
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
        target: &PropertyAddress,
        enabled: bool,
        inspector: &Entity<Self>,
    ) -> Button {
        let inspector = inspector.clone();
        let target = target.clone();
        Self::keyframe_base(
            SharedString::from(format!("toggle-animation-{:?}", target)),
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
        let address = group.target.clone();
        let key = PropertyAddress {
            element_id: Some(edit.element_id()),
            ..group.target.clone()
        };
        Button::new(SharedString::from(format!("array-{key:?}-{suffix}")))
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
    ) -> PropertyRow {
        let property_label = group.property.label().to_owned();
        let rows_are_tuples = matches!(group.property.value_schema(), ValueSchema::Tuple(_));
        let rows_have_scene_binding = group.has_scene_binding;
        let mut rows = div().w_full().min_w_0().flex().flex_col().gap_1();
        for (element_index, row) in group.elements.iter().enumerate() {
            let key = PropertyAddress {
                element_id: Some(row.element_id()),
                ..group.target.clone()
            };
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
                    SharedString::from(format!("bind-scene-array-{key:?}")),
                )
            });
            let mut row_has_binding = is_scene_bound;
            let mut value_rows = if rows_are_tuples {
                Self::property_grid().gap_x_2()
            } else {
                div().min_w_0().flex_1().flex().flex_col().gap_1()
            };
            if group.element_kind != ElementKind::FontFamily {
                for control in row_controls {
                    if rows_are_tuples {
                        if let Some((row, bound)) = Self::scalar_compact_row(control, ctx) {
                            row_has_binding |= bound;
                            value_rows = value_rows.children(row.into_cells());
                        }
                    } else if let Some((row, bound)) = Self::element_scalar(control, ctx) {
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
                    let picker_target = key.clone();
                    let label = if selected_font.is_empty() {
                        t!("rows.choose_font").to_string()
                    } else {
                        selected_font.clone()
                    };
                    let mut trigger = Button::new(SharedString::from(format!("{key:?}-font")))
                        .small()
                        .outline()
                        .w_full()
                        .label(label)
                        .dropdown_caret(true);
                    let trigger_style = trigger.style().clone();
                    value_rows = value_rows.child(
                        Popover::new(SharedString::from(format!("{key:?}-font-picker")))
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
                                                if inspector.set_scalar(
                                                    &target,
                                                    PropertyValue::String(font),
                                                    cx,
                                                ) {
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
                element_index == 0 || rows_have_scene_binding,
                ArrayEdit::MoveUp(element_id),
                ctx,
            );
            let move_down_button = Self::array_edit_button(
                group,
                element_index + 1 == group.elements.len() || rows_have_scene_binding,
                ArrayEdit::MoveDown(element_id),
                ctx,
            );
            let remove_button = Self::array_edit_button(
                group,
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
                                format!("element-label-{key:?}").into(),
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
        let add_address = group.target.clone();
        let next_value = group
            .property
            .element_default_value()
            .expect("array properties declare an element default");
        let add_control = Button::new(SharedString::from(format!("array-{:?}-add", group.target)))
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

        PropertyRow::new(
            div()
                .self_start()
                .child(Self::animation_container_label(
                    property_label,
                    children,
                    ctx,
                    AnimationLabelWidth::Content,
                    format!("group-label-{:?}", group.target).into(),
                ))
                .into_any_element(),
            div()
                .min_w_0()
                .flex()
                .flex_col()
                .gap_1()
                .child(rows)
                .when(allow_structure_edit, |this| {
                    this.child(div().pt_1().child(add_control))
                }),
        )
    }

    /// Array element scalar rendering. Tuple components go through the
    /// shared compact rows; plain scalars keep the array element layout.
    /// Returns each row with whether its scene argument is connected.
    fn element_scalar(control: &Control, ctx: &RenderCtx) -> Option<(gpui::AnyElement, bool)> {
        match control {
            Control::Number(number) if number.common.target.scalar_index.is_some() => {
                let input = ctx.store.text(&number.common.id)?;
                let (row, bound) =
                    Self::number_compact_row(&number.common, &number.spec, &input, ctx);
                Some((row.into_any_element(), bound))
            }
            Control::Number(number) => Self::element_number(number, ctx),
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
                Some((row.into_any_element(), bound))
            }
            Control::Text(_) | Control::Bool(_) | Control::Choice(_) => {
                let row = Self::scalar_full_row(control, ctx)?;
                let bound = control
                    .common()
                    .and_then(|common| common.binding.as_ref())
                    .is_some_and(|binding| binding.connected.is_some());
                Some((row.into_any_element(), bound))
            }
            Control::File(_) => Self::scalar_compact_row(control, ctx)
                .map(|(row, bound)| (row.into_any_element(), bound)),
            Control::Group { .. } => None,
        }
    }

    fn element_number(number: &NumberControl, ctx: &RenderCtx) -> Option<(gpui::AnyElement, bool)> {
        let common = &number.common;
        let spec = &number.spec;
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
                SharedString::from(format!("bind-scene-array-{:?}", common.target)),
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
            .gap_1()
            .when(!component_is_bound, |this| {
                this.child(
                    div()
                        .id(SharedString::from(format!("{:?}", common.target)))
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
