//! Lifetime validation and undo grouping shared by continuous edits.

use super::history::HistoryKey;
use crate::timeline::{SceneId, TimelineEditor};

/// Identifies the editor lifetime in which an edit was resolved. Undo and scene
/// navigation invalidate old targets even when they return to the same document.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct EditContext {
    revision: u64,
    scene_id: SceneId,
}

impl EditContext {
    pub fn is_current(self, editor: &TimelineEditor) -> bool {
        self == editor.edit_context()
    }
}

impl TimelineEditor {
    pub fn edit_context(&self) -> EditContext {
        EditContext {
            revision: self.edit_revision(),
            scene_id: self.active_scene_id(),
        }
    }

    pub fn begin_edit_gesture(&self) -> EditSession {
        EditSession::new(self, UndoPolicy::Gesture)
    }

    pub fn edit_gesture(
        &mut self,
        gesture: &mut EditSession,
        update: impl FnOnce(&mut Self),
    ) -> bool {
        self.update_edit_session(gesture, |editor| {
            let before = editor.edit_context();
            update(editor);
            before != editor.edit_context()
        })
    }
}

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
