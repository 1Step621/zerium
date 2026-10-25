use ::ui::{
    IconName, Sizable as _,
    button::{Button, ButtonVariants as _},
    menu::{PopupMenuItem, popup_menu::PopupMenuExt as _},
};
use gpui::{Entity, SharedString, prelude::*};

use crate::ui::panels::property_inspector::{
    PropertyInspector, SceneFieldBinding, control::ControlMeta, rows::RenderCtx,
};
use rust_i18n::t;

pub(in crate::ui::panels::property_inspector) fn control_binding_button(
    common: &ControlMeta,
    ctx: &RenderCtx,
) -> Option<gpui::AnyElement> {
    common.binding.clone().map(|binding| {
        scene_binding_button(
            binding,
            &ctx.inspector,
            format!("bind-{:?}", common.target).into(),
        )
    })
}

pub(in crate::ui::panels::property_inspector) fn scene_binding_button(
    binding: SceneFieldBinding,
    inspector: &Entity<PropertyInspector>,
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
                let inspector = menu_inspector.clone();
                let argument_id = argument_id.clone();
                let target = binding.target.clone();
                return menu.item(PopupMenuItem::new(t!("rows.unbind").to_string()).on_click(
                    move |_, _, cx| {
                        inspector.update(cx, |inspector, cx| {
                            let result = inspector.editor.update(cx, |editor, cx| {
                                let result =
                                    editor.disconnect_scene_variable(&argument_id, &target);
                                if result.is_ok() {
                                    cx.notify();
                                }
                                result
                            });
                            if result.is_err() {
                                inspector.notifications.update(cx, |notifications, cx| {
                                    notifications.push(t!("rows.unbind_failed").to_string(), cx);
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
