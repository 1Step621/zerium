use std::{
    collections::VecDeque,
    sync::Arc,
    time::{Duration, Instant},
};

use super::{
    EffectInstanceId, Frame, ItemId, PropertyAddress, ResizeEdge, ResizeMode, SceneId,
    TimelineEditor, TimelineTime, project::TimelineProject, selection::SelectionState,
};

#[derive(Clone, Debug, PartialEq)]
pub(super) enum HistoryKey {
    ItemCreation(ItemId),
    Property(ItemId, Option<EffectInstanceId>, String),
    AspectRatioLock(ItemId, Option<EffectInstanceId>),
    AnimationStopValue(PropertyAddress, TimelineTime),
    AnimationRepeat(PropertyAddress),
    Gesture(u64),
    ItemsResize(Vec<ItemId>, ResizeEdge, ResizeMode),
    ItemsMove(Vec<ItemId>),
    SceneName(SceneId),
    SceneArgumentLabel(SceneId, String),
    SceneArgumentSettings(SceneId, String),
}

#[derive(Clone)]
pub(super) struct HistorySnapshot {
    pub(super) project: Arc<TimelineProject>,
    pub(super) scene_path: Vec<SceneId>,
    pub(super) playhead: Frame,
    pub(super) selection: SelectionState,
    pub(super) project_revision: u64,
}

pub(super) type ScopedHistoryKey = (Option<SceneId>, HistoryKey);

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
    /// Explicit gestures stay grouped regardless of pauses between updates.
    pub(super) fn is_current_group(&self, key: &K) -> bool {
        self.last_edit
            .as_ref()
            .is_some_and(|(previous, _)| previous == key)
    }

    pub(super) fn begins_group(&self, key: Option<&K>, max_interval: std::time::Duration) -> bool {
        !key.is_some_and(|key| {
            self.last_edit
                .as_ref()
                .is_some_and(|(previous, at)| previous == key && at.elapsed() <= max_interval)
        })
    }
}

const HISTORY_COALESCE_INTERVAL: Duration = Duration::from_millis(750);

impl TimelineEditor {
    pub(super) fn history_snapshot_for_edit(
        &self,
        key: Option<&HistoryKey>,
    ) -> Option<HistorySnapshot> {
        let scoped_key = self
            .history_group
            .as_ref()
            .or(key)
            .map(|key| (self.active_scene_id(), key.clone()));
        if self.history_group.is_some()
            && scoped_key
                .as_ref()
                .is_some_and(|key| self.history.is_current_group(key))
        {
            return None;
        }
        self.history
            .begins_group(scoped_key.as_ref(), HISTORY_COALESCE_INTERVAL)
            .then(|| self.history_snapshot())
    }

    pub(super) fn finish_project_edit(
        &mut self,
        before: Option<HistorySnapshot>,
        key: Option<HistoryKey>,
    ) {
        let scoped_key = self
            .history_group
            .clone()
            .or(key)
            .map(|key| (self.active_scene_id(), key));
        self.history.record(before, scoped_key);
        self.advance_project_revision();
    }

    /// Own the history and revision boundary; the command reports its actual change.
    pub(super) fn edit_project<T>(
        &mut self,
        key: Option<HistoryKey>,
        update: impl FnOnce(&mut Self) -> (T, bool),
    ) -> T {
        let before = self.history_snapshot_for_edit(key.as_ref());
        let (value, changed) = update(self);
        if changed {
            self.finish_project_edit(before, key);
        }
        value
    }

    pub(super) fn edit_project_if_changed(
        &mut self,
        key: Option<HistoryKey>,
        update: impl FnOnce(&mut Self) -> bool,
    ) -> bool {
        self.edit_project(key, |editor| {
            let changed = update(editor);
            (changed, changed)
        })
    }

    pub(super) fn try_edit_project<T, E>(
        &mut self,
        key: Option<HistoryKey>,
        update: impl FnOnce(&mut Self) -> Result<(T, bool), E>,
    ) -> Result<T, E> {
        self.edit_project(key, |editor| {
            let result = update(editor);
            let changed = result.as_ref().is_ok_and(|(_, changed)| *changed);
            (result.map(|(value, _)| value), changed)
        })
    }

    pub(super) fn edit_project_option<T>(
        &mut self,
        key: Option<HistoryKey>,
        update: impl FnOnce(&mut Self) -> Option<T>,
    ) -> Option<T> {
        self.edit_project(key, |editor| {
            let value = update(editor);
            let changed = value.is_some();
            (value, changed)
        })
    }

    /// Creation and subsequent file initialization share the newly allocated item's group.
    pub(super) fn edit_item_creation<E>(
        &mut self,
        create: impl FnOnce(&mut Self) -> Result<ItemId, E>,
    ) -> Result<ItemId, E> {
        let before = self.history_snapshot();
        let id = create(self)?;
        self.finish_project_edit(Some(before), Some(HistoryKey::ItemCreation(id)));
        Ok(id)
    }
}
