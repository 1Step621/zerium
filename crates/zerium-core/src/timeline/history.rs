use std::{collections::VecDeque, time::Instant};

/// Bounded undo/redo storage with explicit edit coalescing.
///
/// The history owns no domain state itself. Callers provide snapshots at edit
/// boundaries, which keeps persistence, selection, and history policy from
/// becoming coupled to one another.
pub(super) struct EditHistory<S, K> {
    undo: VecDeque<S>,
    redo: VecDeque<S>,
    last_edit: Option<(K, Instant)>,
    limit: usize,
}

impl<S, K> EditHistory<S, K> {
    pub(super) fn new(limit: usize) -> Self {
        assert!(limit > 0, "history limit must be non-zero");
        Self {
            undo: VecDeque::new(),
            redo: VecDeque::new(),
            last_edit: None,
            limit,
        }
    }

    pub(super) fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub(super) fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    pub(super) fn clear(&mut self) {
        self.undo.clear();
        self.redo.clear();
        self.finish_group();
    }

    pub(super) fn finish_group(&mut self) {
        self.last_edit = None;
    }

    fn push(limit: usize, history: &mut VecDeque<S>, snapshot: S) {
        if history.len() == limit {
            history.pop_front();
        }
        history.push_back(snapshot);
    }

    pub(super) fn record(&mut self, before: Option<S>, key: Option<K>) {
        if let Some(before) = before {
            Self::push(self.limit, &mut self.undo, before);
        }
        self.redo.clear();
        self.last_edit = key.map(|key| (key, Instant::now()));
    }

    pub(super) fn undo(&mut self, current: S) -> Option<S> {
        let previous = self.undo.pop_back()?;
        Self::push(self.limit, &mut self.redo, current);
        self.finish_group();
        Some(previous)
    }

    pub(super) fn redo(&mut self, current: S) -> Option<S> {
        let next = self.redo.pop_back()?;
        Self::push(self.limit, &mut self.undo, current);
        self.finish_group();
        Some(next)
    }
}

impl<S, K: PartialEq> EditHistory<S, K> {
    pub(super) fn begins_group(&self, key: Option<&K>, max_interval: std::time::Duration) -> bool {
        !key.is_some_and(|key| {
            self.last_edit
                .as_ref()
                .is_some_and(|(previous, at)| previous == key && at.elapsed() <= max_interval)
        })
    }
}
