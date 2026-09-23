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

    pub(crate) fn set_item_asset(
        &mut self,
        id: ItemId,
        imported: ImportedMedia,
    ) -> Result<(), TimelineEditError> {
        let item = self
            .active_document()
            .item(id)
            .ok_or(TimelineEditError::ItemNotFound(id))?;
        let compatible = item.plugin_id() == Some(imported.plugin_id.as_str())
            && item.item_id() == Some(imported.item_id.as_str())
            && item
                .schema()
                .and_then(|schema| schema.file(&imported.input_id))
                .is_some_and(|file| {
                    file.reader() == imported.asset.reader_id
                        && file.media_type() == imported.asset.kind.media_type()
                });
        if !compatible {
            return Err(TimelineEditError::IncompatibleMedia);
        }
        let key = HistoryKey::ItemCreation(id);
        let before = self.history_snapshot_for_edit(Some(&key));
        let changed = self.active_document_mut().set_item_asset(id, imported);
        if !changed {
            return Err(TimelineEditError::PlacementUnavailable);
        }
        self.finish_project_edit_if_changed(true, before, Some(key));
        Ok(())
    }

    pub(crate) fn remove_item(&mut self, id: ItemId) -> bool {
        let before = self.history_snapshot();
        let changed = self.active_document_mut().remove_item(id);
        if changed {
            self.remove_active_scene_bindings_for(&HashSet::from([id]));
            self.selection.remove(id);
            self.finish_project_edit(Some(before), None);
        }
        changed
    }

    pub(crate) fn remove_selected_item(&mut self) -> bool {
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
        self.selection.primary = None;
        self.selection.remembered_primary = self
            .selection
            .remembered
            .iter()
            .copied()
            .min_by_key(|id| id.get());
        self.finish_project_edit_if_changed(changed, Some(before), None)
    }

    pub(crate) fn paste_items(
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
    pub(crate) fn resize_items(
        &mut self,
        origins: &[TimelineItem],
        anchor_id: ItemId,
        edge: ResizeEdge,
        pointer: Frame,
    ) -> bool {
        if origins.is_empty()
            || origins
                .iter()
                .any(|item| edge == ResizeEdge::Left && item.scene_id().is_some())
        {
            return false;
        }
        let Some(anchor) = origins.iter().find(|item| item.id == anchor_id) else {
            return false;
        };
        let anchor_edge = edge.item_frame(anchor);
        let mut delta = i128::from(pointer.get()) - i128::from(anchor_edge);
        if edge == ResizeEdge::Right {
            for origin in origins {
                let Some(limit) = origin
                    .scene_id()
                    .and_then(|scene_id| self.project().scenes.get(&scene_id))
                    .map(SceneDefinition::duration)
                else {
                    continue;
                };
                let maximum = i128::from(limit.get()) - i128::from(origin.duration.get());
                delta = delta.min(maximum);
            }
        }
        let pointer = (i128::from(anchor_edge) + delta).clamp(0, i128::from(u64::MAX)) as u64;
        let mut ids = origins.iter().map(|item| item.id).collect::<Vec<_>>();
        ids.sort_unstable_by_key(|id| id.get());
        ids.dedup();
        let key = if ids.len() == 1 {
            HistoryKey::ItemResize(ids[0], edge)
        } else {
            HistoryKey::ItemsResize(ids, edge)
        };
        let before = self.history_snapshot_for_edit(Some(&key));
        let changed = self.active_document_mut().resize_items_from(
            origins,
            anchor_id,
            edge,
            Frame::new(pointer),
        );
        self.finish_project_edit_if_changed(changed, before, Some(key))
    }

    pub(crate) fn move_items_from(
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
