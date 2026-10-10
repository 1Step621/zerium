use std::collections::HashSet;

use super::ids::ItemId;

/// Editor-local selection, including the remembered selection restored when
/// the playhead returns to an item's time range.
#[derive(Clone, Default)]
pub(super) struct SelectionState {
    pub(super) current: HashSet<ItemId>,
    pub(super) remembered: HashSet<ItemId>,
}

impl SelectionState {
    pub(super) fn clear(&mut self) {
        self.current.clear();
        self.remembered.clear();
    }

    pub(super) fn select_only(&mut self, id: ItemId) -> bool {
        self.set(HashSet::from([id]))
    }

    pub(super) fn set(&mut self, selected: HashSet<ItemId>) -> bool {
        if self.current == selected && self.remembered == selected {
            return false;
        }
        self.current = selected.clone();
        self.remembered = selected;
        true
    }

    pub(super) fn toggle(&mut self, id: ItemId) -> bool {
        if self.current.contains(&id) {
            self.remove(id);
        } else {
            self.current.insert(id);
            self.remembered.insert(id);
        }
        true
    }

    pub(super) fn sorted_current(&self) -> Vec<ItemId> {
        let mut ids = self.current.iter().copied().collect::<Vec<_>>();
        ids.sort_unstable_by_key(|id| id.get());
        ids
    }

    pub(super) fn remove(&mut self, id: ItemId) {
        self.current.remove(&id);
        self.remembered.remove(&id);
    }

    pub(super) fn restore_where(&mut self, mut is_available: impl FnMut(ItemId) -> bool) -> bool {
        let next = self
            .remembered
            .iter()
            .copied()
            .filter(|id| is_available(*id))
            .collect();
        if self.current == next {
            return false;
        }
        self.current = next;
        true
    }
}
