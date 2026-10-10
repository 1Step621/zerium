use rust_i18n::t;

use super::*;
use crate::ui::copy_buffer::{CopyBuffer, EffectCopy};
use ::ui::menu::PopupMenu;

impl PropertyInspector {
    pub(super) fn effect_menu(
        &self,
        menu: PopupMenu,
        effect_id: Option<EffectInstanceId>,
        cx: &Context<Self>,
    ) -> PopupMenu {
        let editor = self.editor.read(cx);
        let mut menu = menu.action_context(self.focus_handle.clone());
        let Some(item) = self
            .inspector_item_id(cx)
            .and_then(|id| editor.resolved_item(id))
        else {
            return menu;
        };
        if let Some(index) =
            effect_id.and_then(|id| item.effects.iter().position(|effect| effect.id == id))
        {
            let choices = [
                ("clipboard.copy_effect", &item.effects[index..index + 1]),
                ("clipboard.copy_following_effects", &item.effects[index..]),
            ];
            for (label, effects) in choices {
                let copy = Rc::new(EffectCopy::capture(editor, effects));
                menu = menu.menu_handler_with_icon(
                    t!(label).to_string(),
                    IconName::Copy,
                    move |_, cx| {
                        cx.default_global::<CopyBuffer>().effects = Some(copy.clone());
                    },
                );
            }
        }
        let copy = cx
            .try_global::<CopyBuffer>()
            .and_then(|buffer| buffer.effects.clone());
        let inspector = cx.entity();
        let item_id = item.id;
        let session_id = self.session_id;
        let scene_id = editor.active_scene_id();
        menu.when(effect_id.is_some(), |menu| menu.separator())
            .menu_handler_with_icon_and_disabled(
                t!("clipboard.paste_effects").to_string(),
                IconName::PasteClipboard,
                copy.is_none(),
                move |_, cx| {
                    let Some(copy) = &copy else { return };
                    inspector.update(cx, |this, cx| {
                        if !this.session.read(cx).is_current(session_id)
                            || this.editor.read(cx).active_scene_id() != scene_id
                        {
                            return;
                        }
                        let result = this.editor.update(cx, |editor, cx| {
                            let result = editor.paste_effects(item_id, &copy.effects, &copy.hidden);
                            if matches!(result, Ok(true)) {
                                editor.import_media_cache(&copy.media_cache);
                                cx.notify();
                            }
                            result
                        });
                        if let Err(error) = result {
                            this.notifications.update(cx, |notifications, cx| {
                                notifications.push(
                                    t!("clipboard.paste_failed", error = error).to_string(),
                                    cx,
                                );
                            });
                        }
                    });
                },
            )
    }
}
