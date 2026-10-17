use std::collections::HashSet;

use super::ids::ItemId;

/// The user's explicit selection, independent of playhead position.
/// Temporal subsets are derived by TimelineEditor rather than stored here.
#[derive(Clone, Default)]
pub(super) struct SelectionState {
    items: HashSet<ItemId>,
}

impl SelectionState {
    pub(super) fn clear(&mut self) {
        self.items.clear();
    }

    pub(super) fn select_only(&mut self, id: ItemId) -> bool {
        self.set(HashSet::from([id]))
    }

    pub(super) fn set(&mut self, selected: HashSet<ItemId>) -> bool {
        if self.items == selected {
            return false;
        }
        self.items = selected;
        true
    }

    pub(super) fn toggle(&mut self, id: ItemId) -> bool {
        if !self.items.remove(&id) {
            self.items.insert(id);
        }
        true
    }

    pub(super) fn contains(&self, id: ItemId) -> bool {
        self.items.contains(&id)
    }

    pub(super) fn sorted(&self) -> Vec<ItemId> {
        let mut ids = self.items.iter().copied().collect::<Vec<_>>();
        ids.sort_unstable_by_key(|id| id.get());
        ids
    }

    pub(super) fn remove(&mut self, id: ItemId) {
        self.items.remove(&id);
    }
}
