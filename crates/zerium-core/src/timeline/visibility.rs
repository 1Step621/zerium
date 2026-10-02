use std::collections::HashSet;

use super::{
    ids::{EffectInstanceId, ItemId, LayerId},
    item::TimelineItem,
};

/// Non-persistent preview visibility overrides.
///
/// Keeping this separate from the timeline document makes it impossible for
/// solo/mute-style preview choices to leak into project persistence or undo.
#[derive(Clone, Default)]
pub(super) struct PreviewVisibility {
    layers: HashSet<LayerId>,
    items: HashSet<ItemId>,
    effects: HashSet<EffectInstanceId>,
}

impl PreviewVisibility {
    pub(super) fn clear(&mut self) {
        self.layers.clear();
        self.items.clear();
        self.effects.clear();
    }

    pub(super) fn is_item_visible(&self, layer: LayerId, item: ItemId) -> bool {
        !self.layers.contains(&layer) && !self.items.contains(&item)
    }

    pub(super) fn retain_visible_effects(&self, item: &mut TimelineItem) {
        item.effects
            .retain(|effect| !self.effects.contains(&effect.id));
    }

    pub(super) fn hidden_layers(&self) -> impl Iterator<Item = LayerId> + '_ {
        self.layers.iter().copied()
    }

    pub(super) fn hidden_items(&self) -> impl Iterator<Item = ItemId> + '_ {
        self.items.iter().copied()
    }

    pub(super) fn selected_items_hidden_state(&self, selected: &HashSet<ItemId>) -> Option<bool> {
        let mut selected = selected.iter().copied();
        let first = selected.next()?;
        let hidden = self.items.contains(&first);
        selected
            .all(|item_id| self.items.contains(&item_id) == hidden)
            .then_some(hidden)
    }

    pub(super) fn is_effect_hidden(&self, effect: EffectInstanceId) -> bool {
        self.effects.contains(&effect)
    }

    pub(super) fn toggle_layer(&mut self, layer: LayerId) {
        if !self.layers.remove(&layer) {
            self.layers.insert(layer);
        }
    }

    pub(super) fn toggle_items(&mut self, selected: &HashSet<ItemId>) -> bool {
        if selected.is_empty() {
            return false;
        }
        let hide = selected.iter().any(|item_id| !self.items.contains(item_id));
        for item_id in selected {
            if hide {
                self.items.insert(*item_id);
            } else {
                self.items.remove(item_id);
            }
        }
        true
    }

    pub(super) fn toggle_effects(
        &mut self,
        effects: impl IntoIterator<Item = EffectInstanceId>,
    ) -> bool {
        let effects = effects.into_iter().collect::<Vec<_>>();
        if effects.is_empty() {
            return false;
        }
        let hide = effects.iter().any(|effect| !self.effects.contains(effect));
        for effect in effects {
            if hide {
                self.effects.insert(effect);
            } else {
                self.effects.remove(&effect);
            }
        }
        true
    }
}
