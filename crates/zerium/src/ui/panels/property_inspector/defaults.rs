//! Inspector commands capture values; the app owns preference persistence.
use std::collections::BTreeMap;

use ::ui::menu::PopupMenu;
use rust_i18n::t;

use super::*;
use crate::app::{
    defaults::{self, DefaultsTarget},
    storage::config::ConfigStore,
};

impl PropertyInspector {
    pub(super) fn defaults_menu(
        &self,
        menu: PopupMenu,
        effect_id: Option<EffectInstanceId>,
        cx: &Context<Self>,
    ) -> PopupMenu {
        let editor = self.editor.read(cx);
        let Some(item) = self.inspector_item_id(cx).and_then(|id| editor.item(id)) else {
            return menu;
        };
        let Some(target) = DefaultsTarget::for_item(item, effect_id) else {
            return menu;
        };
        let time = TimelineTime::from_frame(editor.playhead());
        let values = editor
            .property_schemas(item, effect_id)
            .filter(|schema| schema.is_editable(None))
            .filter_map(|schema| {
                let address = PropertyAddress {
                    item_id: item.id,
                    effect_id,
                    property_id: schema.id().to_owned(),
                    element_id: None,
                    scalar_index: None,
                };
                let value = editor.evaluated_property_value(&address, time)?;
                schema
                    .accepts_value(&value)
                    .then(|| (schema.id().to_owned(), value))
            })
            .collect::<BTreeMap<_, _>>();
        let context = editor.edit_context();
        let inspector = cx.entity();
        menu.menu_handler(
            t!("inspector.set_type_defaults").to_string(),
            move |_, cx| {
                inspector.update(cx, |this, cx| {
                    if !context.is_current(this.editor.read(cx)) {
                        return;
                    }
                    let result = ConfigStore::update(cx, |config| {
                        config.creation_defaults.store(&target, values.clone());
                    });
                    this.notifications
                        .update(cx, |notifications, cx| match result {
                            Ok(()) => notifications
                                .push_success(t!("inspector.defaults_saved").to_string(), cx),
                            Err(error) => notifications.push(
                                t!("inspector.defaults_save_failed", error = error).to_string(),
                                cx,
                            ),
                        });
                    cx.notify();
                });
            },
        )
    }

    pub(super) fn property_reset_menu(
        &self,
        menu: PopupMenu,
        address: PropertyAddress,
        cx: &Context<Self>,
    ) -> PopupMenu {
        let editor = self.editor.read(cx);
        if editor.item(address.item_id).is_none() {
            return menu;
        }
        let Some(schema) = editor.property_schema(&address) else {
            return menu;
        };
        let disabled = !schema.is_editable(None)
            || self
                .store
                .tree
                .roots
                .iter()
                .any(|control| property_is_driven(control, &address));
        let context = editor.edit_context();
        let inspector = cx.entity();
        menu.menu_handler_with_icon_and_disabled(
            t!("inspector.use_default").to_string(),
            IconName::Undo,
            disabled,
            move |_, cx| {
                inspector.update(cx, |this, cx| {
                    if context.is_current(this.editor.read(cx)) {
                        this.reset_property(address.clone(), cx);
                    }
                })
            },
        )
    }

    pub(super) fn saved_property_default(
        &self,
        address: &PropertyAddress,
        cx: &App,
    ) -> Option<PropertyValue> {
        let editor = self.editor.read(cx);
        let item = editor.item(address.item_id)?;
        let target = DefaultsTarget::for_item(item, address.effect_id)?;
        defaults::property(cx, &target, editor.property_schema(address)?)
    }
}

fn property_is_driven(control: &control::Control, address: &PropertyAddress) -> bool {
    control.common().is_some_and(|common| {
        common.target.item_id == address.item_id
            && common.target.effect_id == address.effect_id
            && common.target.property_id == address.property_id
            && (common.is_bound() || common.animation_enabled)
    }) || control
        .children()
        .any(|child| property_is_driven(child, address))
}
