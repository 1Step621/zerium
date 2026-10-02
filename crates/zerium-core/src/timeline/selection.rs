use std::collections::HashSet;

use super::ids::ItemId;

/// Editor-local selection, including the remembered selection restored when
/// the playhead returns to an item's time range.
#[derive(Clone, Default)]
pub(super) struct SelectionState {
    pub(super) primary: Option<ItemId>,
    pub(super) current: HashSet<ItemId>,
    pub(super) remembered_primary: Option<ItemId>,
    pub(super) remembered: HashSet<ItemId>,
}

impl SelectionState {
    pub(super) fn clear(&mut self) {
        self.primary = None;
        self.current.clear();
        self.remembered_primary = None;
        self.remembered.clear();
    }

    pub(super) fn select_only(&mut self, id: ItemId) -> bool {
        let unchanged = self.primary == Some(id)
            && self.remembered_primary == Some(id)
            && self.current.len() == 1
            && self.remembered.len() == 1
            && self.current.contains(&id)
            && self.remembered.contains(&id);
        if unchanged {
            return false;
        }
        self.primary = Some(id);
        self.current.clear();
        self.current.insert(id);
        self.remembered_primary = Some(id);
        self.remembered.clear();
        self.remembered.insert(id);
        true
    }

    pub(super) fn set(&mut self, selected: HashSet<ItemId>) -> bool {
        let primary = self
            .primary
            .filter(|id| selected.contains(id))
            .or_else(|| selected.iter().copied().min_by_key(|id| id.get()));
        if self.current == selected
            && self.remembered == selected
            && self.primary == primary
            && self.remembered_primary == primary
        {
            return false;
        }
        self.current = selected.clone();
        self.remembered = selected;
        self.primary = primary;
        self.remembered_primary = primary;
        true
    }

    pub(super) fn toggle(&mut self, id: ItemId) -> bool {
        if self.current.contains(&id) {
            self.remove(id);
            return true;
        }
        self.current.insert(id);
        self.remembered.insert(id);
        self.primary = Some(id);
        self.remembered_primary = Some(id);
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
        if self.primary == Some(id) {
            self.primary = self.current.iter().copied().min_by_key(|id| id.get());
        }
        if self.remembered_primary == Some(id) {
            self.remembered_primary = self.remembered.iter().copied().min_by_key(|id| id.get());
        }
    }

    pub(super) fn restore_where(&mut self, mut is_available: impl FnMut(ItemId) -> bool) -> bool {
        let previous_primary = self.primary;
        let previous = self.current.clone();
        self.current = self
            .remembered
            .iter()
            .copied()
            .filter(|id| is_available(*id))
            .collect();
        self.primary = self
            .remembered_primary
            .filter(|id| self.current.contains(id))
            .or_else(|| self.current.iter().copied().min_by_key(|id| id.get()));
        previous_primary != self.primary || previous != self.current
    }
}
