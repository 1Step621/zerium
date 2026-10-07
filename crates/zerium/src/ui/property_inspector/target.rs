use rust_i18n::t;

use super::*;

impl PropertyInspector {
    pub(super) fn target_selector(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let editor = self.editor.read(cx);
        let items = editor.selected_items();
        let multiple = items.len() > 1;
        let mut options = Vec::new();
        if multiple {
            options.push((
                EditScope::Selection,
                t!("inspector.common_target").to_string(),
            ));
        }
        for item in items {
            let scope = if !multiple {
                EditScope::Selection
            } else {
                EditScope::Item(item.id)
            };
            let label = t!(
                "inspector.item_option",
                name = editor.item_label(item.id).unwrap_or_default(),
                layer = editor
                    .item_layer(item.id)
                    .expect("selected item has a layer")
                    .get()
                    + 1,
                frame = item.start.get()
            )
            .to_string();
            options.push((scope, label));
        }
        let current = if !multiple {
            EditScope::Selection
        } else {
            self.scope
        };
        let label = options
            .iter()
            .find(|(scope, _)| *scope == current)
            .map(|(_, label)| label.clone())
            .unwrap_or_default();
        let inspector = cx.entity();
        Button::new("inspector-item-selector")
            .small()
            .compact()
            .outline()
            .dropdown_caret(true)
            .label(label)
            .tooltip(t!("inspector.item_target").to_string())
            .popup_menu(move |menu, _, _| {
                options.iter().fold(menu, |menu, (scope, label)| {
                    let scope = *scope;
                    let inspector = inspector.clone();
                    menu.item(
                        PopupMenuItem::new(label.clone())
                            .checked(scope == current)
                            .on_click(move |_, window, cx| {
                                inspector.update(cx, |this, cx| this.set_scope(scope, window, cx));
                            }),
                    )
                })
            })
            .into_any_element()
    }

    fn set_scope(&mut self, scope: EditScope, window: &mut Window, cx: &mut Context<Self>) {
        if self.scope == scope
            || matches!(scope, EditScope::Item(id) if !self.editor.read(cx).is_item_selected(id))
        {
            return;
        }
        self.finish_number_drag(cx);
        self.scope = scope;
        self.effect_picker = None;
        self.reset_input_state();
        // Follow the same property on the displayed item when possible. The
        // timeline selection, which defines synchronization, is never changed.
        let candidates = self
            .animation_selection
            .read(cx)
            .address()
            .map(|address| AnimationSelection::candidates_for(address, self.editor.read(cx)))
            .unwrap_or_default()
            .into_iter()
            .filter(|address| {
                scope == EditScope::Selection || scope == EditScope::Item(address.item_id)
            })
            .collect();
        self.animation_selection.update(cx, |selection, cx| {
            selection.focus_candidates(candidates, cx)
        });
        let editor = self.editor.clone();
        self.sync_from_editor(&editor, window, cx);
    }
}
