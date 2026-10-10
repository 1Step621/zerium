use std::collections::HashSet;

use crate::timeline::{
    EffectInstanceId, Frame, FrameRate, ItemId, LayerId, ProjectResolution, SceneId,
    TimelineDocument, TimelineEditor, TimelineTime,
};

// Editor session, history, selection, and preview commands.
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

    pub fn open_scene(&mut self, id: SceneId) -> bool {
        if !self.project().scenes.contains_key(&id) || self.scene_path.last() == Some(&id) {
            return false;
        }
        self.scene_path.push(id);
        self.history.finish_group();
        self.selection.clear();
        self.active_edit_target = None;
        self.visibility.clear();
        self.playhead = Frame::new(0);
        self.playback_time = None;
        self.advance_render_revision();
        true
    }

    pub fn close_scene(&mut self) -> bool {
        if self.scene_path.pop().is_none() {
            return false;
        }
        self.history.finish_group();
        self.selection.clear();
        self.active_edit_target = None;
        self.visibility.clear();
        self.playhead = Frame::new(0);
        self.playback_time = None;
        self.advance_render_revision();
        true
    }

    pub fn clear_playback_time(&mut self) -> bool {
        if self.playback_time.take().is_none() {
            return false;
        }
        self.advance_render_revision();
        true
    }

    pub fn set_playback_position(&mut self, seconds: f64, frame: Frame) -> bool {
        let Some(time) = TimelineTime::from_seconds(seconds, self.frame_rate()) else {
            return false;
        };
        if time.nearest_frame() != frame {
            return false;
        }
        if self.playback_time == Some(time) && self.playhead == frame {
            return false;
        }
        self.playback_time = Some(time);
        self.playhead = frame;
        self.advance_render_revision();
        true
    }

    pub fn toggle_layer_visibility(&mut self, layer: LayerId) -> bool {
        self.visibility.toggle_layer(layer);
        self.advance_render_revision();
        true
    }

    pub fn toggle_selected_items_visibility(&mut self) -> bool {
        if !self
            .visibility
            .toggle_items(self.selection.sorted_current())
        {
            return false;
        }
        self.advance_render_revision();
        true
    }

    pub fn toggle_item_visibility(&mut self, id: ItemId) -> bool {
        if !self.is_item_selected(id) {
            return false;
        }
        self.visibility.toggle_items([id]);
        self.advance_render_revision();
        true
    }

    pub fn toggle_effect_visibility(
        &mut self,
        item_id: ItemId,
        effect_id: EffectInstanceId,
    ) -> bool {
        if !self.is_item_selected(item_id)
            || self
                .item(item_id)
                .is_none_or(|item| item.effect(effect_id).is_none())
        {
            return false;
        }
        self.visibility.toggle_effects([effect_id]);
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

    pub fn seek(&mut self, frame: Frame) -> bool {
        let old_playhead = self.playhead;
        self.playhead = frame;
        let playback_changed = self.playback_time.take().is_some();
        let selectable = self
            .active_document()
            .items()
            .filter(|item| item.contains(frame))
            .map(|item| item.id)
            .collect::<HashSet<_>>();
        let selection_changed = self.selection.restore_where(|id| selectable.contains(&id));

        if old_playhead != self.playhead || playback_changed {
            self.advance_render_revision();
        }

        old_playhead != self.playhead || playback_changed || selection_changed
    }

    pub fn set_playhead(&mut self, frame: Frame) -> bool {
        let playback_changed = self.playback_time.take().is_some();
        if self.playhead == frame && !playback_changed {
            return false;
        }
        self.playhead = frame;
        self.advance_render_revision();
        true
    }

    pub fn step_playhead(&mut self, delta: i64) -> bool {
        let next = if delta < 0 {
            self.playhead.0.saturating_sub(delta.unsigned_abs())
        } else {
            self.playhead.0.saturating_add(delta as u64)
        };
        self.set_playhead(Frame(next))
    }
}
