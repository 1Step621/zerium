use super::{
    PropertyRow, RenderCtx, color_row, element_number, number_compact_row, property_grid,
    scalar_compact_row, scalar_full_row,
};
use crate::ui::{
    components::search_picker::{SearchPicker, SearchPickerEntry},
    panels::property_inspector::{
        control::{ArrayControl, Control, ElementKind},
        edit_array,
        editing::ArrayEdit,
        push_element,
        render::{
            animation::{
                AnimationLabelWidth, animation_container_label, animation_label_base,
                animation_property_label, control_contains_focused_animation,
            },
            binding::scene_binding_button,
        },
    },
};
use ::ui::{
    Disableable as _, IconName, Sizable as _,
    button::{Button, ButtonVariants as _},
    popover::Popover,
};
use gpui::{SharedString, div, prelude::*, px};

use zerium_core::{property::PropertyValue, timeline::PropertyAddress};

use rust_i18n::t;

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
                if edit_array(editor, &address, edit) {
                    cx.notify();
                }
            });
        })
}

pub(in crate::ui::panels::property_inspector) fn elements_section(
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
                row_controls
                    .iter()
                    .find_map(|control| control.common().and_then(|common| common.binding.clone()))
            })
            .flatten();
        let is_scene_bound = scene_binding
            .as_ref()
            .is_some_and(|binding| binding.connected.is_some());
        let binding_button = scene_binding.map(|binding| {
            scene_binding_button(
                binding,
                &ctx.inspector,
                SharedString::from(format!("bind-scene-array-{key:?}")),
            )
        });
        let mut row_has_binding = is_scene_bound;
        let mut value_rows = if rows_are_tuples {
            property_grid().gap_x_2()
        } else {
            div().min_w_0().flex_1().flex().flex_col().gap_1()
        };
        if group.element_kind != ElementKind::FontFamily {
            for control in row_controls {
                if rows_are_tuples {
                    if let Some((row, bound)) = scalar_compact_row(control, ctx) {
                        row_has_binding |= bound;
                        value_rows = value_rows.children(row.into_cells());
                    }
                } else if let Some((row, bound)) = element_scalar(control, ctx) {
                    row_has_binding |= bound;
                    value_rows = value_rows.child(row);
                }
            }
        }

        match group.element_kind {
            ElementKind::Scalar => {}
            ElementKind::FontFamily => {
                let Some(selected_font) = font_name(row) else {
                    continue;
                };
                let font_choices = ctx
                    .font_names
                    .iter()
                    .filter(|font| {
                        !group.rows.iter().enumerate().any(|(index, value)| {
                            index != element_index && font_name(value) == Some(font.as_str())
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
        let move_up_button = array_edit_button(
            group,
            element_index == 0 || rows_have_scene_binding,
            ArrayEdit::MoveUp(element_id),
            ctx,
        );
        let move_down_button = array_edit_button(
            group,
            element_index + 1 == group.rows.len() || rows_have_scene_binding,
            ArrayEdit::MoveDown(element_id),
            ctx,
        );
        let remove_button = array_edit_button(
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
                        .child(animation_container_label(
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
                if push_element(editor, &add_address, next_value.clone()) {
                    cx.notify();
                }
            });
        });

    PropertyRow::new(
        div()
            .self_start()
            .child(
                animation_label_base(
                    property_label.into(),
                    AnimationLabelWidth::Content,
                    group
                        .rows
                        .iter()
                        .flat_map(|row| &row.controls)
                        .any(|control| control_contains_focused_animation(control, ctx)),
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
            let input = ctx.store.inputs.text(&number.common.id())?;
            let (row, bound) = number_compact_row(&number.common, &number.spec, &input, ctx);
            Some((row.into_any_element(), bound))
        }
        Control::Number(number) => element_number(number, ctx),
        Control::Color(color) => {
            let common = &color.common;
            let picker = ctx.store.inputs.color(&common.id())?;
            let bound = common.is_bound();
            let row = color_row(
                color,
                &picker,
                Some(animation_property_label(common.label.clone(), common, ctx)),
                ctx,
            );
            Some((row.into_any_element(), bound))
        }
        Control::Text(_) | Control::Bool(_) | Control::Choice(_) => {
            let row = scalar_full_row(control, ctx)?;
            let bound = control
                .common()
                .and_then(|common| common.binding.as_ref())
                .is_some_and(|binding| binding.connected.is_some());
            Some((row.into_any_element(), bound))
        }
        Control::File(_) => {
            scalar_compact_row(control, ctx).map(|(row, bound)| (row.into_any_element(), bound))
        }
        Control::Tuple(_) | Control::Array(_) | Control::Effect(_) => None,
    }
}

fn font_name(row: &crate::ui::panels::property_inspector::control::ArrayRow) -> Option<&str> {
    row.controls.iter().find_map(|control| match control {
        Control::Text(text) => Some(text.value.as_str()),
        _ => None,
    })
}
