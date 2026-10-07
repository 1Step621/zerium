use super::*;

// Item, effect, animation, and layout commands.
impl TimelineEditor {
    fn remove_active_scene_bindings_for(&mut self, item_ids: &HashSet<ItemId>) {
        let Some(scene_id) = self.active_scene_id() else {
            return;
        };
        let Some(scene) = self.project_mut().scenes.get_mut(&scene_id) else {
            return;
        };
        remove_bindings_for_items(scene, item_ids);
    }

    /// Initialize a newly placed item from an imported source. File edits on
    /// existing items go through the ordinary property commands.
    pub fn import_item_file(
        &mut self,
        id: ItemId,
        imported: ImportedFile,
    ) -> Result<(), TimelineEditError> {
        let item = self
            .active_document()
            .item(id)
            .ok_or(TimelineEditError::ItemNotFound(id))?;
        let compatible = item.plugin_id() == Some(imported.plugin_id.as_str())
            && item.item_id() == Some(imported.source_id.as_str())
            && item
                .schema()
                .and_then(|schema| schema.file_property(&imported.property_id))
                .is_some();
        if !compatible {
            return Err(TimelineEditError::IncompatibleMedia);
        }
        let key = Some(HistoryKey::ItemCreation(id));
        let before = self.history_snapshot_for_edit(key.as_ref());
        let file = imported.file.clone();
        let changed = self.active_document_mut().import_item_file(id, imported);
        if !changed {
            return Err(TimelineEditError::PlacementUnavailable);
        }
        self.cache_media_file(&file);
        self.finish_project_edit_if_changed(true, before, key);
        Ok(())
    }

    pub fn remove_item(&mut self, id: ItemId) -> bool {
        let before = self.history_snapshot();
        let changed = self.active_document_mut().remove_item(id);
        if changed {
            self.remove_active_scene_bindings_for(&HashSet::from([id]));
            self.selection.remove(id);
            self.finish_project_edit(Some(before), None);
        }
        changed
    }

    pub fn remove_selected_item(&mut self) -> bool {
        if self.selection.current.is_empty() {
            return false;
        }
        let before = self.history_snapshot();
        let selected = std::mem::take(&mut self.selection.current);
        let mut changed = false;
        for id in &selected {
            changed |= self.active_document_mut().remove_item(*id);
            self.selection.remembered.remove(id);
        }
        self.remove_active_scene_bindings_for(&selected);
        self.finish_project_edit_if_changed(changed, Some(before), None)
    }

    pub fn paste_items(
        &mut self,
        sources: &[(LayerId, TimelineItem)],
        source_scene: Option<SceneId>,
        source_bindings: &[(String, SceneBindingTarget)],
        target_layer: LayerId,
        target_start: Frame,
    ) -> Option<Vec<ItemId>> {
        if sources.is_empty()
            || sources.iter().any(|(_, item)| {
                item.scene_id()
                    .is_some_and(|scene_id| !self.can_add_scene_instance(scene_id))
            })
        {
            return None;
        }

        let before = self.history_snapshot();
        let active_scene = self.active_scene_id();
        let mut next_project = self.project().clone();
        let mut copies = sources.to_vec();
        let mut next_effect_id = self.next_effect_id;
        let mut effect_ids = HashMap::new();
        for (_, item) in &mut copies {
            for effect in &mut item.effects {
                let old_id = effect.id;
                let raw_id = next_effect_id?;
                if raw_id == u64::MAX {
                    return None;
                }
                let new_id = EffectInstanceId::new(raw_id);
                next_effect_id = raw_id.checked_add(1).filter(|id| *id != u64::MAX);
                effect.id = new_id;
                effect_ids.insert(old_id, new_id);
            }
        }

        let document = match active_scene {
            Some(scene_id) => next_project.scenes.get_mut(&scene_id)?.document_mut(),
            None => &mut next_project.document,
        };
        let item_ids = document.insert_item_copies(&copies, target_layer, target_start)?;
        let item_id_map = item_ids.iter().copied().collect::<HashMap<_, _>>();

        if source_scene == active_scene
            && let Some(scene_id) = active_scene
        {
            let scene = next_project.scenes.get_mut(&scene_id)?;
            for (argument_id, binding) in source_bindings {
                let item_id = item_id_map.get(&binding.item_id()).copied()?;
                let effect_id = match binding.owner() {
                    SceneBindingOwner::Item => None,
                    SceneBindingOwner::Effect(effect_id) => Some(*effect_ids.get(&effect_id)?),
                };
                let remapped = SceneBindingTarget::new(
                    item_id,
                    SceneBindingOwner::from_effect(effect_id),
                    binding.property_id(),
                    binding.element_id(),
                    binding.scalar_index(),
                );
                let argument = scene
                    .arguments
                    .iter_mut()
                    .find(|argument| argument.schema.id() == argument_id)?;
                if argument.bindings.contains(&remapped) {
                    return None;
                }
                argument.bindings.push(remapped);
            }
            let mut targets = HashSet::new();
            if scene
                .arguments
                .iter()
                .flat_map(|argument| &argument.bindings)
                .any(|binding| !targets.insert(binding.clone()))
            {
                return None;
            }
        }

        let pasted = item_ids
            .into_iter()
            .map(|(_, new_id)| new_id)
            .collect::<Vec<_>>();
        self.commit_project_state(next_project);
        self.next_effect_id = next_effect_id;
        self.selection.set(pasted.iter().copied().collect());
        self.finish_project_edit(Some(before), None);
        Some(pasted)
    }

    /// Apply one scalar edit to each selected instance while retaining its own siblings.
    pub fn resize_items(
        &mut self,
        origins: &[TimelineItem],
        anchor_id: ItemId,
        edge: ResizeEdge,
        pointer: Frame,
        mode: ResizeMode,
    ) -> bool {
        if origins.is_empty()
            || origins
                .iter()
                .any(|item| edge == ResizeEdge::Left && item.scene_id().is_some())
        {
            return false;
        }
        let mut ids = origins.iter().map(|item| item.id).collect::<Vec<_>>();
        ids.sort_unstable_by_key(|id| id.get());
        ids.dedup();
        let key = if ids.len() == 1 {
            HistoryKey::ItemResize(ids[0], edge, mode)
        } else {
            HistoryKey::ItemsResize(ids, edge, mode)
        };
        let before = self.history_snapshot_for_edit(Some(&key));
        let changed = self
            .active_document_mut()
            .resize_items_from(origins, anchor_id, edge, pointer, mode);
        self.finish_project_edit_if_changed(changed, before, Some(key))
    }

    pub fn move_items_from(
        &mut self,
        origins: &[(ItemId, Frame, LayerId)],
        frame_delta: i64,
        layer_delta: i64,
    ) -> bool {
        let mut ids = origins.iter().map(|(id, _, _)| *id).collect::<Vec<_>>();
        ids.sort_unstable_by_key(|id| id.get());
        ids.dedup();
        if ids.len() != origins.len() {
            return false;
        }
        let key = if ids.len() == 1 {
            HistoryKey::ItemMove(ids[0])
        } else {
            HistoryKey::ItemsMove(ids)
        };
        let before = self.history_snapshot_for_edit(Some(&key));
        let changed = self
            .active_document_mut()
            .move_items_from(origins, frame_delta, layer_delta);
        self.finish_project_edit_if_changed(changed, before, Some(key))
    }
}
