use crate::ui::panels::property_inspector::{
    PropertyInspector, control::ControlMeta, render::binding::control_binding_button,
};
use ::ui::{
    Disableable as _, Sizable as _,
    button::Button,
    menu::{PopupMenuItem, popup_menu::PopupMenuExt as _},
    switch::Switch,
};
use gpui::{Entity, SharedString, div, prelude::*};

use zerium_core::{property::PropertyValue, timeline::PropertyAddress};

use super::{PropertyRow, RenderCtx};
use rust_i18n::t;

pub(in crate::ui::panels::property_inspector) fn toggle_row(
    common: &ControlMeta,
    value: bool,
    label: Option<gpui::AnyElement>,
    ctx: &RenderCtx,
) -> PropertyRow {
    let is_bound = common.is_bound();
    let binding_button = control_binding_button(common, ctx);
    let switch = bool_switch(&common.target, value, common.read_only, &ctx.inspector);
    PropertyRow::new(
        label,
        div()
            .min_w_0()
            .flex()
            .items_center()
            .gap_2()
            .when(!is_bound, |this| this.child(switch))
            .when_some(binding_button, |this, button| this.child(button)),
    )
}

fn bool_switch(
    target: &PropertyAddress,
    value: bool,
    read_only: bool,
    inspector: &Entity<PropertyInspector>,
) -> Switch {
    let checked = value;
    let inspector = inspector.clone();
    let target = target.clone();
    Switch::new(SharedString::from(format!("{target:?}")))
        .small()
        .checked(checked)
        .disabled(read_only)
        .tooltip(if checked {
            t!("edit.turn_off")
        } else {
            t!("edit.turn_on")
        })
        .on_click(move |checked, _, cx| {
            inspector.update(cx, |inspector, cx| {
                let changed = inspector.set_scalar(&target, PropertyValue::Bool(*checked), cx);
                if changed {
                    cx.notify();
                }
            });
        })
}

pub(in crate::ui::panels::property_inspector) fn dropdown_row(
    common: &ControlMeta,
    current: u32,
    options: &[(String, u32)],
    label: Option<gpui::AnyElement>,
    ctx: &RenderCtx,
) -> PropertyRow {
    let bound = common.is_bound();
    let binding_button = control_binding_button(common, ctx);
    PropertyRow::new(
        label,
        div()
            .min_w_0()
            .flex()
            .items_center()
            .gap_1()
            .when(!bound, |row| {
                row.child(choice_dropdown(
                    &common.target,
                    current,
                    options,
                    common.read_only,
                    &ctx.inspector,
                ))
            })
            .when_some(binding_button, |row, button| row.child(button)),
    )
}

fn choice_dropdown(
    target: &PropertyAddress,
    current: u32,
    options: &[(String, u32)],
    read_only: bool,
    inspector: &Entity<PropertyInspector>,
) -> impl IntoElement {
    let selected_label = options
        .iter()
        .find(|(_, value)| *value == current)
        .map(|(label, _)| label.clone())
        .unwrap_or_default();
    let options = options.to_vec();
    let inspector = inspector.clone();
    let target = target.clone();
    Button::new(SharedString::from(format!("{target:?}")))
        .small()
        .outline()
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
                        let changed = inspector.set_scalar(&target, PropertyValue::Enum(value), cx);
                        if changed {
                            cx.notify();
                        }
                    });
                }))
            })
        })
}
