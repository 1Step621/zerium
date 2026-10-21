use crate::ui::property_inspector::rows::{
    animation_property_label, animation_scalar_label, scene_binding_button,
};
use ::ui::{
    Disableable as _, Sizable as _,
    button::Button,
    menu::{PopupMenuItem, popup_menu::PopupMenuExt as _},
    switch::Switch,
};
use gpui::{Entity, SharedString, div, prelude::*};

use zerium_core::property::PropertyValue;
use zerium_core::timeline::PropertyAddress;

use crate::ui::property_inspector::control::ControlMeta;
use crate::ui::property_inspector::rows::{PropertyRow, RenderCtx};
use crate::ui::property_inspector::{PropertyInspector, SceneFieldBinding};
use rust_i18n::t;

pub(in crate::ui::property_inspector) fn toggle_full_row(
    common: &ControlMeta,
    value: bool,
    binding: Option<SceneFieldBinding>,
    ctx: &RenderCtx,
) -> PropertyRow {
    let is_bound = binding
        .as_ref()
        .is_some_and(|binding| binding.connected.is_some());
    let binding_button = binding.map(|binding| {
        scene_binding_button(
            binding,
            &ctx.inspector,
            SharedString::from(format!("bind-scene-argument-{:?}", common.target)),
        )
    });
    let switch = bool_switch(&common.target, value, common.read_only, &ctx.inspector);
    PropertyRow::new(
        animation_property_label(common.label.clone(), common, ctx),
        div()
            .min_w_0()
            .flex()
            .items_center()
            .gap_2()
            .when(!is_bound, |this| this.child(switch))
            .when_some(binding_button, |this, button| this.child(button)),
    )
}

pub(in crate::ui::property_inspector) fn toggle_compact_row(
    common: &ControlMeta,
    value: bool,
    binding: Option<SceneFieldBinding>,
    ctx: &RenderCtx,
) -> (PropertyRow, bool) {
    let is_bound = binding
        .as_ref()
        .is_some_and(|binding| binding.connected.is_some());
    let binding_button = binding.map(|binding| {
        scene_binding_button(
            binding,
            &ctx.inspector,
            SharedString::from(format!("bind-scene-argument-{:?}", common.target)),
        )
    });
    let switch = bool_switch(&common.target, value, common.read_only, &ctx.inspector);
    let row = PropertyRow::new(
        animation_scalar_label(common, ctx),
        div()
            .min_w_0()
            .flex()
            .items_center()
            .gap_2()
            .when(!is_bound, |this| this.child(switch))
            .when_some(binding_button, |this, button| this.child(button)),
    );
    (row, is_bound)
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

pub(in crate::ui::property_inspector) fn dropdown_full_row(
    common: &ControlMeta,
    current: u32,
    options: &[(String, u32)],
    binding: Option<SceneFieldBinding>,
    ctx: &RenderCtx,
) -> PropertyRow {
    let bound = binding
        .as_ref()
        .is_some_and(|binding| binding.connected.is_some());
    let binding_button = binding.map(|binding| {
        scene_binding_button(
            binding,
            &ctx.inspector,
            SharedString::from(format!("bind-{:?}", common.target)),
        )
    });
    PropertyRow::new(
        animation_property_label(common.label.clone(), common, ctx),
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

pub(in crate::ui::property_inspector) fn dropdown_compact_row(
    common: &ControlMeta,
    current: u32,
    options: &[(String, u32)],
    binding: Option<SceneFieldBinding>,
    ctx: &RenderCtx,
) -> (PropertyRow, bool) {
    let bound = binding
        .as_ref()
        .is_some_and(|binding| binding.connected.is_some());
    let binding_button = binding.map(|binding| {
        scene_binding_button(
            binding,
            &ctx.inspector,
            SharedString::from(format!("bind-{:?}", common.target)),
        )
    });
    let row = PropertyRow::new(
        animation_scalar_label(common, ctx),
        div()
            .min_w_0()
            .flex()
            .items_center()
            .gap_1()
            .when(!bound, |this| {
                this.child(choice_dropdown(
                    &common.target,
                    current,
                    options,
                    common.read_only,
                    &ctx.inspector,
                ))
            })
            .when_some(binding_button, |this, button| this.child(button)),
    );
    (row, bound)
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
