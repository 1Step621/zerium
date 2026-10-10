use rust_i18n::t;

use super::*;

impl PropertyInspector {
    pub(super) fn target_selector(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let editor = self.editor.read(cx);
        let options = editor
            .selected_item_ids()
            .filter_map(|id| {
                let item = editor.item(id)?;
                let label = t!(
                    "inspector.item_option",
                    name = editor.item_label(id).unwrap_or_default(),
                    layer = editor.item_layer(id)?.get() + 1,
                    frame = item.start.get(),
                )
                .to_string();
                Some((id, label))
            })
            .collect::<Vec<_>>();
        let current = self.item_id;
        let label = options
            .iter()
            .find(|(id, _)| Some(*id) == current)
            .map(|(_, label)| label.clone())
            .unwrap_or_default();
        let inspector = cx.entity();
        Button::new("inspector-item-selector")
            .small()
            .compact()
            .ghost()
            .dropdown_caret(true)
            .label(label)
            .tooltip(t!("inspector.item_target").to_string())
            .popup_menu(move |menu, _, _| {
                options.iter().fold(menu, |menu, (item_id, label)| {
                    let item_id = *item_id;
                    let inspector = inspector.clone();
                    menu.item(
                        PopupMenuItem::new(label.clone())
                            .checked(Some(item_id) == current)
                            .on_click(move |_, window, cx| {
                                inspector.update(cx, |this, cx| this.set_item(item_id, window, cx));
                            }),
                    )
                })
            })
            .into_any_element()
    }

    fn set_item(&mut self, item_id: ItemId, window: &mut Window, cx: &mut Context<Self>) {
        if self.item_id == Some(item_id) || !self.editor.read(cx).is_item_selected(item_id) {
            return;
        }
        self.finish_number_drag(cx);
        self.item_id = Some(item_id);
        self.effect_picker = None;
        self.reset_input_state();
        // Follow the same property without changing timeline selection or synchronization.
        self.animation_selection.update(cx, |selection, cx| {
            selection.select_item(item_id, &self.editor, cx)
        });
        self.sync_from_editor(window, cx);
    }
}
