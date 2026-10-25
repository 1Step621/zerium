use std::collections::HashSet;

use crate::timeline::model::ItemId;

/// The user's explicit selection, independent of playhead position.
/// Temporal subsets are derived by TimelineEditor rather than stored here.
#[derive(Clone, Default)]
pub(in crate::timeline) struct SelectionState {
    items: HashSet<ItemId>,
}

impl SelectionState {
    pub(in crate::timeline) fn clear(&mut self) {
        self.items.clear();
    }

    pub(in crate::timeline) fn select_only(&mut self, id: ItemId) -> bool {
        self.set(HashSet::from([id]))
    }

    pub(in crate::timeline) fn set(&mut self, selected: HashSet<ItemId>) -> bool {
        if self.items == selected {
            return false;
        }
        self.items = selected;
        true
    }

    pub(in crate::timeline) fn toggle(&mut self, id: ItemId) -> bool {
        if !self.items.remove(&id) {
            self.items.insert(id);
        }
        true
    }

    pub(in crate::timeline) fn contains(&self, id: ItemId) -> bool {
        self.items.contains(&id)
    }

    pub(in crate::timeline) fn sorted(&self) -> Vec<ItemId> {
        let mut ids = self.items.iter().copied().collect::<Vec<_>>();
        ids.sort_unstable_by_key(|id| id.get());
        ids
    }

    pub(in crate::timeline) fn remove(&mut self, id: ItemId) {
        self.items.remove(&id);
    }
}
