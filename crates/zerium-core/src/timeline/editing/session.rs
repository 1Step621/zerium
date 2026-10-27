use std::collections::HashSet;

use super::PlayheadPosition;
use crate::timeline::{
    Frame, FrameRate, ItemId, ProjectResolution, SceneId, TimelineDocument, TimelineEditor,
    TimelineTime,
};

// Editor session, history, selection, and navigation.
impl TimelineEditor {
    pub fn undo(&mut self) -> bool {
        let current = self.history_snapshot();
        let Some(snapshot) = self.history.undo(current) else {
            return false;
        };
        self.restore_history_snapshot(snapshot);
        true
    }

    pub fn redo(&mut self) -> bool {
        let current = self.history_snapshot();
        let Some(snapshot) = self.history.redo(current) else {
            return false;
        };
        self.restore_history_snapshot(snapshot);
        true
    }

    pub fn finish_history_group(&mut self) {
        self.history.finish_group();
    }

    pub fn reset(&mut self, resolution: ProjectResolution, frame_rate: FrameRate, playhead: Frame) {
        self.replace_document(TimelineDocument::new(frame_rate), resolution, playhead);
    }

    /// Scope-local state must be reset together whenever navigation changes documents.
    pub(in crate::timeline) fn reset_scene_session(&mut self) {
        self.selection.clear();
        self.active_edit_target = None;
        self.position = PlayheadPosition::Stopped(Frame::new(0));
    }

    fn finish_scene_navigation(&mut self) {
        self.reset_scene_session();
        self.history.finish_group();
        self.advance_edit_revision();
        self.advance_render_revision();
    }

    pub fn open_scene(&mut self, id: SceneId) -> bool {
        if !self.project().scenes.contains_key(&id) || self.active_scene_id() == id {
            return false;
        }
        if id == self.main_scene_id() {
            self.scene_path.clear();
        } else {
            self.scene_path.push(id);
        }
        self.finish_scene_navigation();
        true
    }

    pub fn close_scene(&mut self) -> bool {
        if self.scene_path.pop().is_none() {
            return false;
        }
        self.finish_scene_navigation();
        true
    }

    pub fn clear_playback_time(&mut self) -> bool {
        self.set_playhead(self.playhead())
    }

    pub fn set_playback_position(&mut self, seconds: f64, frame: Frame) -> bool {
        let Some(time) = TimelineTime::from_seconds(seconds, self.frame_rate()) else {
            return false;
        };
        if time.nearest_frame() != frame {
            return false;
        }
        let position = PlayheadPosition::Playing(time);
        if self.position == position {
            return false;
        }
        self.position = position;
        self.advance_render_revision();
        true
    }

    pub fn select(&mut self, id: ItemId) -> bool {
        if self.active_document().item(id).is_none() {
            return false;
        }
        self.selection.select_only(id)
    }

    pub fn toggle_item_selection(&mut self, id: ItemId) -> bool {
        if self.active_document().item(id).is_none() {
            return false;
        }
        self.selection.toggle(id)
    }

    pub fn select_items(&mut self, ids: impl IntoIterator<Item = ItemId>) -> bool {
        let selected_items = ids
            .into_iter()
            .filter(|id| self.active_document().item(*id).is_some())
            .collect::<HashSet<_>>();
        self.selection.set(selected_items)
    }

    pub fn set_playhead(&mut self, frame: Frame) -> bool {
        let position = PlayheadPosition::Stopped(frame);
        if self.position == position {
            return false;
        }
        self.position = position;
        self.advance_render_revision();
        true
    }

    pub fn step_playhead(&mut self, delta: i64) -> bool {
        let next = if delta < 0 {
            self.playhead().get().saturating_sub(delta.unsigned_abs())
        } else {
            self.playhead().get().saturating_add(delta as u64)
        };
        self.set_playhead(Frame::new(next))
    }
}
