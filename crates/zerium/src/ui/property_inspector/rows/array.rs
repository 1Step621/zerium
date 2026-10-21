use ::ui::{
    Disableable as _, IconName, Sizable as _,
    button::{Button, ButtonVariants as _},
    popover::Popover,
};
use gpui::{SharedString, div, prelude::*, px};

use crate::ui::search_picker::{SearchPicker, SearchPickerEntry};
use zerium_core::property::PropertyValue;
use zerium_core::timeline::PropertyAddress;

use crate::ui::property_inspector::PropertyInspector;
use crate::ui::property_inspector::control::{ArrayControl, Control, ElementKind};
use crate::ui::property_inspector::edit::ArrayEdit;
use crate::ui::property_inspector::rows::{PropertyRow, RenderCtx, animation::AnimationLabelWidth};
use rust_i18n::t;

impl PropertyInspector {
    fn array_edit_button(
        group: &ArrayControl,
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

    pub(in crate::ui::property_inspector) fn elements_section(
        group: &ArrayControl,
        ctx: &RenderCtx,
        separator_color: gpui::Hsla,
        allow_structure_edit: bool,
    ) -> PropertyRow {
        let property_label = group.property.label().to_owned();
        let rows_are_tuples = group.property.value_schema().tuple().is_some();
        let rows_have_scene_binding = group.has_scene_binding;
        let mut rows = div().w_full().min_w_0().flex().flex_col().gap_1();
        for (element_index, row) in group.rows.iter().enumerate() {
            let key = PropertyAddress {
                element_id: Some(row.element_id),
                ..group.target.clone()
            };
            let row_controls = row.controls.as_slice();
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
                    let Some(selected_font) = row.font() else {
                        continue;
                    };
                    let font_choices = ctx
                        .font_names
                        .iter()
                        .filter(|font| {
                            !group.rows.iter().enumerate().any(|(index, value)| {
                                index != element_index && value.font() == Some(font.as_str())
                            })
                        })
                        .map(|font| SearchPickerEntry::new(font.clone(), "", font.clone()))
                        .collect::<Vec<_>>();
                    let picker_inspector = ctx.inspector.clone();
                    let picker_target = key.clone();
                    let label = if selected_font.is_empty() {
                        t!("rows.choose_font").to_string()
                    } else {
                        selected_font.to_owned()
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

            let element_id = row.element_id;
            let move_up_button = Self::array_edit_button(
                group,
                element_index == 0 || rows_have_scene_binding,
                ArrayEdit::MoveUp(element_id),
                ctx,
            );
            let move_down_button = Self::array_edit_button(
                group,
                element_index + 1 == group.rows.len() || rows_have_scene_binding,
                ArrayEdit::MoveDown(element_id),
                ctx,
            );
            let remove_button = Self::array_edit_button(
                group,
                group.rows.len() <= group.min_items as usize
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

        let add_disabled = group.rows.len() >= group.max_items as usize || rows_have_scene_binding;
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
                .child(
                    Self::animation_label_base(
                        property_label.into(),
                        AnimationLabelWidth::Content,
                        group
                            .rows
                            .iter()
                            .flat_map(|row| &row.controls)
                            .any(|control| Self::control_contains_focused_animation(control, ctx)),
                        ctx,
                    )
                    .id(SharedString::from(format!(
                        "group-label-{:?}",
                        group.target
                    ))),
                )
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
                let common = &color.common;
                let picker = ctx.store.color(&common.id)?;
                let bound = common
                    .binding
                    .as_ref()
                    .is_some_and(|binding| binding.connected.is_some());
                let row = Self::color_full_row(
                    color,
                    &picker,
                    common.animation_enabled,
                    common.binding.clone(),
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
            Control::Tuple(_) | Control::Array(_) | Control::Effect(_) => None,
        }
    }
}
