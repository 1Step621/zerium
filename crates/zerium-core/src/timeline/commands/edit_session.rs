//! Operation lifetime and Undo policy shared by every continuous edit.
use super::EditContext;
use crate::timeline::{TimelineEditor, history::HistoryKey};

pub(super) enum UndoPolicy {
    Gesture,
    Coalesced(HistoryKey),
}

/// Tracks one edit operation and rejects updates after unrelated edits.
/// Gesture and coalesced edits share the same revision and Undo bookkeeping.
pub struct EditSession {
    context: EditContext,
    key: HistoryKey,
    changed: bool,
}

impl EditSession {
    pub(super) fn new(editor: &TimelineEditor, policy: UndoPolicy) -> Self {
        let context = editor.edit_context();
        let key = match policy {
            UndoPolicy::Gesture => HistoryKey::Gesture(context.revision),
            UndoPolicy::Coalesced(key) => key,
        };
        Self {
            context,
            key,
            changed: false,
        }
    }

    pub fn is_current(&self, editor: &TimelineEditor) -> bool {
        self.context.is_current(editor)
            && (!matches!(self.key, HistoryKey::Gesture(_))
                || !self.changed
                || editor
                    .history
                    .is_current_group(&(self.context.scene_id, self.key.clone())))
    }

    fn continues_group(&self, editor: &TimelineEditor) -> bool {
        matches!(self.key, HistoryKey::Gesture(_))
            || (self.changed
                && editor
                    .history
                    .is_current_group(&(self.context.scene_id, self.key.clone())))
    }
}

impl TimelineEditor {
    pub(super) fn update_edit_session(
        &mut self,
        session: &mut EditSession,
        update: impl FnOnce(&mut Self) -> bool,
    ) -> bool {
        if !session.is_current(self) || self.history_group.is_some() {
            return false;
        }
        if session.continues_group(self) {
            self.history_group = Some(session.key.clone());
        }
        let changed = update(self);
        self.history_group = None;
        if changed {
            session.context = self.edit_context();
            session.changed = true;
        }
        changed
    }
}
