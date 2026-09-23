use super::*;

// Editor session, history, selection, and preview commands.
impl TimelineEditor {
    pub(crate) fn undo(&mut self) -> bool {
        let current = self.history_snapshot();
        let Some(snapshot) = self.history.undo(current) else {
            return false;
        };
        self.restore_history_snapshot(snapshot);
        true
    }

    pub(crate) fn redo(&mut self) -> bool {
        let current = self.history_snapshot();
        let Some(snapshot) = self.history.redo(current) else {
            return false;
        };
        self.restore_history_snapshot(snapshot);
        true
    }

    pub(crate) fn finish_history_group(&mut self) {
        self.history.finish_group();
    }

    pub(crate) fn reset(
        &mut self,
        resolution: ProjectResolution,
        frame_rate: FrameRate,
        playhead: Frame,
    ) {
        self.replace_document(TimelineDocument::new(frame_rate), resolution, playhead);
    }

    pub(crate) fn open_scene(&mut self, id: SceneId) -> bool {
        if !self.project().scenes.contains_key(&id) || self.scene_path.last() == Some(&id) {
            return false;
        }
        self.scene_path.push(id);
        self.history.finish_group();
        self.selection.clear();
        self.visibility.clear();
        self.playhead = Frame::new(0);
        self.playback_time = None;
        self.advance_render_revision();
        true
    }

    pub(crate) fn close_scene(&mut self) -> bool {
        if self.scene_path.pop().is_none() {
            return false;
        }
        self.history.finish_group();
        self.selection.clear();
        self.visibility.clear();
        self.playhead = Frame::new(0);
        self.playback_time = None;
        self.advance_render_revision();
        true
    }

    pub(crate) fn clear_playback_time(&mut self) -> bool {
        if self.playback_time.take().is_none() {
            return false;
        }
        self.advance_render_revision();
        true
    }

    pub(crate) fn set_playback_position(&mut self, seconds: f64, frame: Frame) -> bool {
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

    pub(crate) fn toggle_layer_visibility(&mut self, layer: LayerId) -> bool {
        self.visibility.toggle_layer(layer);
        self.advance_render_revision();
        true
    }

    pub(crate) fn toggle_selected_items_visibility(&mut self) -> bool {
        if !self.visibility.toggle_items(&self.selection.current) {
            return false;
        }
        self.advance_render_revision();
        true
    }

    pub(crate) fn toggle_selected_effect_visibility(
        &mut self,
        primary_effect_id: EffectInstanceId,
    ) -> bool {
        let Some(effects) = self.selected_effect_instances(primary_effect_id) else {
            return false;
        };
        if !self
            .visibility
            .toggle_effects(effects.into_iter().map(|(_, effect_id)| effect_id))
        {
            return false;
        }
        self.advance_render_revision();
        true
    }

    pub(crate) fn move_selected_effect(
        &mut self,
        primary_effect_id: EffectInstanceId,
        offset: i32,
    ) -> bool {
        if !self.can_move_selected_effect(primary_effect_id, offset) {
            return false;
        }
        let Some(primary_item_id) = self.selection.primary else {
            return false;
        };
        let Some(source_index) = self
            .active_document()
            .item(primary_item_id)
            .and_then(|item| {
                item.effects
                    .iter()
                    .position(|effect| effect.id == primary_effect_id)
            })
        else {
            return false;
        };
        let Some(target_index) = source_index.checked_add_signed(offset as isize) else {
            return false;
        };
        let Some(effects) = self.selected_effect_instances(primary_effect_id) else {
            return false;
        };
        let before = self.history_snapshot();
        let mut changed = false;
        for (item_id, effect_id) in effects {
            changed |=
                self.active_document_mut()
                    .move_item_effect(item_id, effect_id, target_index);
        }
        self.finish_project_edit_if_changed(changed, Some(before), None)
    }

    pub(crate) fn select(&mut self, id: ItemId) -> bool {
        if self.active_document().item(id).is_none() {
            return false;
        }
        self.selection.select_only(id)
    }

    pub(crate) fn toggle_item_selection(&mut self, id: ItemId) -> bool {
        if self.active_document().item(id).is_none() {
            return false;
        }
        self.selection.toggle(id)
    }

    pub(crate) fn select_items(&mut self, ids: impl IntoIterator<Item = ItemId>) -> bool {
        let selected_items = ids
            .into_iter()
            .filter(|id| self.active_document().item(*id).is_some())
            .collect::<HashSet<_>>();
        self.selection.set(selected_items)
    }

    pub(crate) fn seek(&mut self, frame: Frame) -> bool {
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

    pub(crate) fn set_playhead(&mut self, frame: Frame) -> bool {
        let playback_changed = self.playback_time.take().is_some();
        if self.playhead == frame && !playback_changed {
            return false;
        }
        self.playhead = frame;
        self.advance_render_revision();
        true
    }

    pub(crate) fn step_playhead(&mut self, delta: i64) -> bool {
        let next = if delta < 0 {
            self.playhead.0.saturating_sub(delta.unsigned_abs())
        } else {
            self.playhead.0.saturating_add(delta as u64)
        };
        self.set_playhead(Frame(next))
    }

    pub(crate) fn add_item(
        &mut self,
        layer: LayerId,
        start: Frame,
        plugin_id: &str,
        item_id: &str,
    ) -> Result<ItemId, TimelineEditError> {
        let schema = self.plugins.item(plugin_id, item_id).ok_or_else(|| {
            TimelineEditError::PluginItemNotFound {
                plugin_id: plugin_id.to_owned(),
                item_id: item_id.to_owned(),
            }
        })?;
        let before = self.history_snapshot();
        let id = self
            .active_document_mut()
            .add_item(layer, start, plugin_id, item_id, schema)
            .ok_or(TimelineEditError::PlacementUnavailable)?;
        self.selection.select_only(id);
        self.finish_project_edit(Some(before), Some(HistoryKey::ItemCreation(id)));
        Ok(id)
    }
}
