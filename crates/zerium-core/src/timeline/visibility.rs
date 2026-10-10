use std::{collections::HashSet, hash::Hash};

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

    pub(super) fn items_hidden_state(
        &self,
        items: impl IntoIterator<Item = ItemId>,
    ) -> Option<bool> {
        let mut selected = items.into_iter();
        let first = selected.next()?;
        let hidden = self.items.contains(&first);
        selected
            .all(|item_id| self.items.contains(&item_id) == hidden)
            .then_some(hidden)
    }

    pub(super) fn is_item_hidden(&self, item: ItemId) -> bool {
        self.items.contains(&item)
    }

    pub(super) fn is_effect_hidden(&self, effect: EffectInstanceId) -> bool {
        self.effects.contains(&effect)
    }

    pub(super) fn toggle_layer(&mut self, layer: LayerId) {
        if !self.layers.remove(&layer) {
            self.layers.insert(layer);
        }
    }

    pub(super) fn toggle_items(&mut self, items: impl IntoIterator<Item = ItemId>) -> bool {
        toggle_group(&mut self.items, items)
    }

    pub(super) fn toggle_effects(
        &mut self,
        effects: impl IntoIterator<Item = EffectInstanceId>,
    ) -> bool {
        toggle_group(&mut self.effects, effects)
    }
}

fn toggle_group<T: Eq + Hash>(
    hidden: &mut HashSet<T>,
    targets: impl IntoIterator<Item = T>,
) -> bool {
    let targets = targets.into_iter().collect::<Vec<_>>();
    if targets.is_empty() {
        return false;
    }
    let hide = targets.iter().any(|target| !hidden.contains(target));
    for target in targets {
        if hide {
            hidden.insert(target);
        } else {
            hidden.remove(&target);
        }
    }
    true
}
