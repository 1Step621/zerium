use std::collections::{HashMap, HashSet};

use crate::{
    media::ImportedFile,
    timeline::{
        BlendMode, EffectInstanceId, Frame, ItemId, LayerId, ResizeEdge, ResizeMode,
        SceneBindingOwner, SceneBindingTarget, SceneId, TimelineEditError, TimelineEditor,
        TimelineItem,
        editing::history::{EditImpact, HistoryKey},
    },
};

use super::scene::remove_bindings_for_items;

// Item, effect, animation, and layout commands.
impl TimelineEditor {
    pub fn set_item_blend_mode(&mut self, id: ItemId, mode: BlendMode) -> bool {
        if !self.is_item_selected(id) {
            return false;
        }
        self.edit_project_if_changed(EditImpact::Values, None, |editor| {
            let item = editor
                .active_document_mut()
                .item_mut(id)
                .expect("selected item must exist");
            if item.blend_mode == mode {
                return false;
            }
            item.blend_mode = mode;
            true
        })
    }

    pub fn add_item(
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
        self.edit_item_creation(|editor| {
            let id = editor
                .active_document_mut()
                .add_item(layer, start, plugin_id, item_id, schema)
                .ok_or(TimelineEditError::PlacementUnavailable)?;
            editor.selection.select_only(id);
            Ok(id)
        })
    }

    fn remove_active_scene_bindings_for(&mut self, item_ids: &HashSet<ItemId>) {
        remove_bindings_for_items(self.active_scene_mut(), item_ids);
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
        self.try_edit_project(EditImpact::MediaDependencies, key, |editor| {
            let file = imported.file.clone();
            let changed = editor.active_document_mut().import_item_file(id, imported);
            if !changed {
                return Err(TimelineEditError::PlacementUnavailable);
            }
            editor.cache_media_file(&file);
            Ok(((), true))
        })
    }

    pub fn remove_item(&mut self, id: ItemId) -> bool {
        self.edit_project_if_changed(EditImpact::MediaDependencies, None, |editor| {
            let changed = editor.active_document_mut().remove_item(id);
            if changed {
                editor.remove_active_scene_bindings_for(&HashSet::from([id]));
                editor.selection.remove(id);
            }
            changed
        })
    }

    pub fn remove_selected_item(&mut self) -> bool {
        let selected = self.selected_item_ids().collect::<HashSet<_>>();
        if selected.is_empty() {
            return false;
        }
        self.edit_project_if_changed(EditImpact::MediaDependencies, None, |editor| {
            let mut changed = false;
            for id in &selected {
                changed |= editor.active_document_mut().remove_item(*id);
                editor.selection.remove(*id);
            }
            editor.remove_active_scene_bindings_for(&selected);
            changed
        })
    }

    pub fn paste_items(
        &mut self,
        sources: &[(LayerId, TimelineItem)],
        source_scene: Option<SceneId>,
        source_bindings: &[(String, SceneBindingTarget)],
        source_hidden_effects: &[EffectInstanceId],
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

        self.edit_project_option(EditImpact::MediaDependencies, None, |editor| {
            let active_scene = editor.active_scene_id();
            let mut copies = sources.to_vec();
            let mut next_effect_id = editor.next_effect_id;
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

            let document = editor.active_document_mut();
            let item_ids = document.insert_item_copies(&copies, target_layer, target_start)?;
            let item_id_map = item_ids.iter().copied().collect::<HashMap<_, _>>();

            if source_scene == Some(active_scene) {
                let scene = editor.active_scene_mut();
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
                        .variables_mut()
                        .iter_mut()
                        .find(|argument| argument.id() == argument_id)?;
                    if argument.bindings.contains(&remapped) {
                        return None;
                    }
                    argument.bindings.push(remapped);
                }
                let mut targets = HashSet::new();
                if scene
                    .variables()
                    .iter()
                    .flat_map(|argument| &argument.bindings)
                    .any(|binding| !targets.insert(binding.clone()))
                {
                    return None;
                }
            }

            // Clipboard references are untrusted and may outlive schema edits.
            // Validate connections against the copied items before committing.
            {
                let scene = editor.active_scene();
                for variable in scene.variables() {
                    for binding in &variable.bindings {
                        let resolved = crate::timeline::resolve_scene_binding(
                            &editor.project().scenes,
                            scene,
                            binding,
                        )?;
                        if resolved.animated
                            || !resolved.schema.is_scene_bindable(None)
                            || !resolved
                                .schema
                                .scalar_type(None, None)
                                .is_some_and(|ty| ty.same_type(variable.ty()))
                        {
                            return None;
                        }
                    }
                }
            }

            let pasted = item_ids
                .into_iter()
                .map(|(_, new_id)| new_id)
                .collect::<Vec<_>>();
            editor.next_effect_id = next_effect_id;
            editor.visibility.toggle_effects(
                source_hidden_effects
                    .iter()
                    .filter_map(|id| effect_ids.get(id).copied()),
            );
            editor.selection.set(pasted.iter().copied().collect());
            Some(pasted)
        })
    }

    /// Resize the selected intervals from their gesture origins.
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
        let key = HistoryKey::ItemsResize(ids, edge, mode);
        let Some(edits) = self
            .active_document()
            .prepare_resize_items_from(origins, anchor_id, edge, pointer, mode)
        else {
            return false;
        };
        self.commit_item_edits(Some(key), edits).unwrap_or(false)
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
        let key = HistoryKey::ItemsMove(ids);
        let Some(edits) =
            self.active_document()
                .prepare_move_items_from(origins, frame_delta, layer_delta)
        else {
            return false;
        };
        self.commit_item_edits(Some(key), edits).unwrap_or(false)
    }
}
