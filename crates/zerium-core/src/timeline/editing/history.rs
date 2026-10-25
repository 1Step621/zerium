use std::{
    collections::VecDeque,
    sync::Arc,
    time::{Duration, Instant},
};

use super::selection::SelectionState;
use crate::timeline::{
    EffectInstanceId, Frame, ItemId, PropertyAddress, ResizeEdge, ResizeMode, SceneId,
    TimelineEditor, TimelineTime, model::project::TimelineProject,
};

/// Media discovery follows authored file references, independently of rendering.
#[derive(Clone, Copy)]
pub(in crate::timeline) enum EditImpact {
    Values,
    MediaDependencies,
}

#[derive(Clone, Debug, PartialEq)]
pub(in crate::timeline) enum HistoryKey {
    ItemCreation(ItemId),
    Property(ItemId, Option<EffectInstanceId>, String),
    AspectRatioLock(ItemId, Option<EffectInstanceId>),
    AnimationStopValue(PropertyAddress, usize),
    AnimationStopInsertion(PropertyAddress, TimelineTime),
    AnimationRepeat(PropertyAddress),
    Gesture(u64),
    ItemsResize(Vec<ItemId>, ResizeEdge, ResizeMode),
    SceneName(SceneId),
    SceneStretchPoint(SceneId),
    SceneVariableLabel(SceneId, String),
    SceneArgumentSettings(SceneId, String),
    SceneCalculationCode(SceneId, String),
    SceneCalculationInputs(SceneId, String),
}

#[derive(Clone)]
pub(in crate::timeline) struct HistorySnapshot {
    pub(in crate::timeline) project: Arc<TimelineProject>,
    pub(in crate::timeline) scene_path: Vec<SceneId>,
    pub(in crate::timeline) playhead: Frame,
    pub(in crate::timeline) selection: SelectionState,
    pub(in crate::timeline) project_revision: u64,
}

pub(in crate::timeline) type ScopedHistoryKey = (SceneId, HistoryKey);

/// Bounded undo/redo storage with explicit edit coalescing.
///
/// The history owns no domain state itself. Callers provide snapshots at edit
/// boundaries, which keeps persistence, selection, and history policy from
/// becoming coupled to one another.
pub(in crate::timeline) struct EditHistory<S, K> {
    undo: VecDeque<S>,
    redo: VecDeque<S>,
    last_edit: Option<(K, Instant)>,
    limit: usize,
}

impl<S, K> EditHistory<S, K> {
    pub(in crate::timeline) fn new(limit: usize) -> Self {
        assert!(limit > 0, "history limit must be non-zero");
        Self {
            undo: VecDeque::new(),
            redo: VecDeque::new(),
            last_edit: None,
            limit,
        }
    }

    pub(in crate::timeline) fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub(in crate::timeline) fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    pub(in crate::timeline) fn clear(&mut self) {
        self.undo.clear();
        self.redo.clear();
        self.finish_group();
    }

    pub(in crate::timeline) fn finish_group(&mut self) {
        self.last_edit = None;
    }

    fn push(limit: usize, history: &mut VecDeque<S>, snapshot: S) {
        if history.len() == limit {
            history.pop_front();
        }
        history.push_back(snapshot);
    }

    pub(in crate::timeline) fn record(&mut self, before: Option<S>, key: Option<K>) {
        if let Some(before) = before {
            Self::push(self.limit, &mut self.undo, before);
        }
        self.redo.clear();
        self.last_edit = key.map(|key| (key, Instant::now()));
    }

    pub(in crate::timeline) fn undo(&mut self, current: S) -> Option<S> {
        let previous = self.undo.pop_back()?;
        Self::push(self.limit, &mut self.redo, current);
        self.finish_group();
        Some(previous)
    }

    pub(in crate::timeline) fn redo(&mut self, current: S) -> Option<S> {
        let next = self.redo.pop_back()?;
        Self::push(self.limit, &mut self.undo, current);
        self.finish_group();
        Some(next)
    }
}

impl<S, K: PartialEq> EditHistory<S, K> {
    /// Explicit gestures stay grouped regardless of pauses between updates.
    pub(in crate::timeline) fn is_current_group(&self, key: &K) -> bool {
        self.last_edit
            .as_ref()
            .is_some_and(|(previous, _)| previous == key)
    }

    pub(in crate::timeline) fn begins_group(
        &self,
        key: Option<&K>,
        max_interval: std::time::Duration,
    ) -> bool {
        !key.is_some_and(|key| {
            self.last_edit
                .as_ref()
                .is_some_and(|(previous, at)| previous == key && at.elapsed() <= max_interval)
        })
    }
}

const HISTORY_COALESCE_INTERVAL: Duration = Duration::from_millis(750);

impl TimelineEditor {
    pub(in crate::timeline) fn history_snapshot_for_edit(
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

    fn finish_project_edit(
        &mut self,
        before: Option<HistorySnapshot>,
        key: Option<HistoryKey>,
        impact: EditImpact,
    ) {
        if matches!(impact, EditImpact::MediaDependencies) {
            self.advance_media_dependencies_revision();
        }
        let scoped_key = self
            .history_group
            .clone()
            .or(key)
            .map(|key| (self.active_scene_id(), key));
        self.history.record(before, scoped_key);
        self.advance_project_revision();
    }

    /// Fast path for validated item-only changes: no whole-project rollback copy.
    pub(in crate::timeline) fn commit_item_edits(
        &mut self,
        key: Option<HistoryKey>,
        edits: impl IntoIterator<Item = crate::timeline::model::document::ItemEdit>,
    ) -> Result<bool, crate::timeline::TimelineEditError> {
        let prepared = self.active_document().prepare_item_edits(edits)?;
        if prepared.is_empty() {
            return Ok(false);
        }
        let media_dependencies_changed = prepared.media_dependencies_changed;
        let before = self.history_snapshot_for_edit(key.as_ref());
        self.active_document_mut().publish_item_edits(prepared);
        let impact = if media_dependencies_changed {
            EditImpact::MediaDependencies
        } else {
            EditImpact::Values
        };
        self.finish_project_edit(before, key, impact);
        Ok(true)
    }

    /// Own the history and revision boundary; the command reports its actual change.
    pub(in crate::timeline) fn edit_project<T>(
        &mut self,
        impact: EditImpact,
        key: Option<HistoryKey>,
        update: impl FnOnce(&mut Self) -> (T, bool),
    ) -> T {
        let prepared = self.prepare_project_edit();
        let before = self.history_snapshot_for_edit(key.as_ref());
        let (value, changed) = update(self);
        self.complete_project_edit(prepared, before, key, impact, changed);
        value
    }

    pub(in crate::timeline) fn edit_project_if_changed(
        &mut self,
        impact: EditImpact,
        key: Option<HistoryKey>,
        update: impl FnOnce(&mut Self) -> bool,
    ) -> bool {
        self.edit_project(impact, key, |editor| {
            let changed = update(editor);
            (changed, changed)
        })
    }

    pub(in crate::timeline) fn try_edit_project<T, E>(
        &mut self,
        impact: EditImpact,
        key: Option<HistoryKey>,
        update: impl FnOnce(&mut Self) -> Result<(T, bool), E>,
    ) -> Result<T, E> {
        self.edit_project(impact, key, |editor| {
            let result = update(editor);
            let changed = result.as_ref().is_ok_and(|(_, changed)| *changed);
            (result.map(|(value, _)| value), changed)
        })
    }

    pub(in crate::timeline) fn edit_project_option<T>(
        &mut self,
        impact: EditImpact,
        key: Option<HistoryKey>,
        update: impl FnOnce(&mut Self) -> Option<T>,
    ) -> Option<T> {
        self.edit_project(impact, key, |editor| {
            let value = update(editor);
            let changed = value.is_some();
            (value, changed)
        })
    }

    fn complete_project_edit(
        &mut self,
        prepared: crate::timeline::editing::ProjectEditState,
        before: Option<HistorySnapshot>,
        key: Option<HistoryKey>,
        impact: EditImpact,
        changed: bool,
    ) {
        if changed {
            self.finish_project_edit(before, key, impact);
        } else {
            self.discard_project_edit(prepared);
        }
    }

    /// Creation and subsequent file initialization share the newly allocated item's group.
    pub(in crate::timeline) fn edit_item_creation<E>(
        &mut self,
        create: impl FnOnce(&mut Self) -> Result<ItemId, E>,
    ) -> Result<ItemId, E> {
        let prepared = self.prepare_project_edit();
        let before = self.history_snapshot();
        let result = create(self);
        let key = result.as_ref().ok().map(|id| HistoryKey::ItemCreation(*id));
        self.complete_project_edit(
            prepared,
            Some(before),
            key,
            EditImpact::MediaDependencies,
            result.is_ok(),
        );
        result
    }
}
