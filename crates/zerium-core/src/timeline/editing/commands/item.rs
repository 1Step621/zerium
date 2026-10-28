use std::collections::{HashMap, HashSet};

use crate::{
    media::ImportedFile,
    property::PropertyOverrides,
    timeline::{
        BlendMode, EffectInstanceId, Frame, FrameDuration, FrameRate, ItemCreation, ItemId,
        ItemInterval, ItemSource, LayerId, ResizeEdge, ResizeMode, SceneBindingOwner,
        SceneBindingTarget, SceneId, TimelineEditError, TimelineEditor, TimelineItem,
        editing::{
            gesture::EditSession,
            history::{EditImpact, HistoryKey},
        },
        model::document::{CopyPlacement, ItemEdit},
    },
};

use super::scene::remove_bindings_for_items;

/// A move gesture owns its original placements and any copies until release.
pub struct ItemMoveEdit {
    origins: Vec<(LayerId, TimelineItem)>,
    copies: Option<HashMap<ItemId, ItemId>>,
    session: EditSession,
}

impl ItemMoveEdit {
    pub fn is_current(&self, editor: &TimelineEditor) -> bool {
        self.session.is_current(editor)
    }

    pub fn origins(&self) -> &[(LayerId, TimelineItem)] {
        &self.origins
    }

    pub fn clamp_delta(
        &self,
        frame_delta: i64,
        layer_delta: i64,
        frame_rate: FrameRate,
    ) -> (i64, i64) {
        let (min_start, max_end, min_layer, max_layer) = self
            .origins
            .iter()
            .map(|(layer, item)| {
                (
                    item.start().get(),
                    item.end_exclusive(frame_rate).get(),
                    layer.get(),
                    layer.get(),
                )
            })
            .reduce(|a, b| (a.0.min(b.0), a.1.max(b.1), a.2.min(b.2), a.3.max(b.3)))
            .unwrap_or((0, 0, 0, 0));
        let clamp = |delta: i64, minimum: u64, maximum: u64| {
            i128::from(delta).clamp(-i128::from(minimum), i128::from(u64::MAX - maximum)) as i64
        };
        (
            clamp(frame_delta, min_start, max_end),
            clamp(layer_delta, min_layer, max_layer),
        )
    }

    pub fn snap_excluded_items(&self, duplicate: bool) -> impl Iterator<Item = ItemId> + '_ {
        self.origins
            .iter()
            .filter_map(move |(_, item)| (!duplicate || self.copies.is_none()).then_some(item.id))
            .chain(
                self.copies
                    .iter()
                    .flat_map(|copies| copies.values().copied()),
            )
    }
}

// Item, effect, animation, and layout commands.
impl TimelineEditor {
    pub fn toggle_layer_visibility(&mut self, layer: LayerId) -> bool {
        self.edit_project_if_changed(EditImpact::Values, None, |editor| {
            editor.active_document_mut().toggle_layer_visibility(layer);
            true
        })
    }

    pub fn toggle_item_visibility(&mut self, id: ItemId) -> bool {
        if !self.is_item_selected(id) {
            return false;
        }
        let Ok(mut edit) = self.active_document().prepare_item_edit(id) else {
            return false;
        };
        edit.item.hidden = !edit.item.hidden;
        self.commit_item_edits(None, [edit]).unwrap_or(false)
    }

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
        properties: PropertyOverrides,
    ) -> Result<ItemId, TimelineEditError> {
        let schema = self.plugins.item(plugin_id, item_id).ok_or_else(|| {
            TimelineEditError::PluginItemNotFound {
                plugin_id: plugin_id.to_owned(),
                item_id: item_id.to_owned(),
            }
        })?;
        crate::timeline::model::item::property::validate_initial_properties(
            &properties,
            schema.properties(),
        )?;
        self.edit_item_creation(|editor| {
            let id = editor
                .active_document_mut()
                .add_item(
                    layer,
                    plugin_id,
                    schema,
                    ItemCreation {
                        start,
                        duration: None,
                        properties,
                    },
                )
                .ok_or(TimelineEditError::PlacementUnavailable)?;
            editor.selection.select_only(id);
            Ok(id)
        })
    }

    /// Insert a batch at exact requested positions as one atomic, undoable edit.
    pub fn add_items(
        &mut self,
        layer: LayerId,
        source: &ItemSource,
        items: Vec<ItemCreation>,
    ) -> Result<Vec<ItemId>, TimelineEditError> {
        if items.is_empty() {
            return Ok(Vec::new());
        }
        let plugin = match source {
            ItemSource::Plugin { plugin_id, item_id } => {
                Some(self.plugins.item(plugin_id, item_id).ok_or_else(|| {
                    TimelineEditError::PluginItemNotFound {
                        plugin_id: plugin_id.clone(),
                        item_id: item_id.clone(),
                    }
                })?)
            }
            ItemSource::Scene(scene_id) => {
                if self.scene(*scene_id).is_none() {
                    return Err(TimelineEditError::SceneNotFound(*scene_id));
                }
                if !self.can_add_scene_instance(*scene_id) {
                    return Err(TimelineEditError::RecursiveSceneReference);
                }
                None
            }
        };
        let schemas = match source {
            ItemSource::Plugin { .. } => plugin
                .as_ref()
                .expect("resolved plugin")
                .properties()
                .to_vec(),
            ItemSource::Scene(scene_id) => self
                .scene(*scene_id)
                .expect("resolved scene")
                .arguments()
                .filter_map(|argument| argument.argument_schema().cloned())
                .collect(),
        };
        for item in &items {
            crate::timeline::model::item::property::validate_initial_properties(
                &item.properties,
                &schemas,
            )?;
        }
        self.try_edit_project(EditImpact::MediaDependencies, None, |editor| {
            let mut ids = Vec::with_capacity(items.len());
            for initial in items {
                let start = initial.start;
                let id = match source {
                    ItemSource::Plugin { plugin_id, .. } => editor.active_document_mut().add_item(
                        layer,
                        plugin_id,
                        plugin.clone().expect("resolved plugin"),
                        initial,
                    ),
                    ItemSource::Scene(scene_id) => {
                        let duration = initial.duration.unwrap_or_else(|| {
                            editor.scene(*scene_id).expect("resolved scene").duration()
                        });
                        editor.active_document_mut().insert_item(
                            layer,
                            start,
                            duration,
                            |id, start, duration| {
                                let mut item =
                                    TimelineItem::scene_instance(id, start, duration, *scene_id);
                                item.properties = initial.properties;
                                item
                            },
                        )
                    }
                }
                .ok_or(TimelineEditError::PlacementUnavailable)?;
                if editor.item(id).expect("inserted item").start() != start {
                    return Err(TimelineEditError::PlacementUnavailable);
                }
                ids.push(id);
            }
            editor.selection.set(ids.iter().copied().collect());
            Ok((ids, true))
        })
    }

    /// Import media consecutively as one atomic, undoable edit.
    pub fn import_items(
        &mut self,
        layer: LayerId,
        start: Frame,
        imports: Vec<(ImportedFile, PropertyOverrides)>,
    ) -> Result<Vec<ItemId>, TimelineEditError> {
        if imports.is_empty() {
            return Ok(Vec::new());
        }
        let files = imports
            .iter()
            .map(|(imported, _)| imported.file.clone())
            .collect::<Vec<_>>();
        let ids = self.try_edit_project(EditImpact::MediaDependencies, None, |editor| {
            let mut start = start;
            let mut ids = Vec::with_capacity(imports.len());
            for (imported, properties) in imports {
                let schema = editor
                    .plugins
                    .item(&imported.plugin_id, &imported.source_id)
                    .ok_or_else(|| TimelineEditError::PluginItemNotFound {
                        plugin_id: imported.plugin_id.clone(),
                        item_id: imported.source_id.clone(),
                    })?;
                crate::timeline::model::item::property::validate_initial_properties(
                    &properties,
                    schema.properties(),
                )?;
                let id = editor
                    .active_document_mut()
                    .add_item(
                        layer,
                        &imported.plugin_id,
                        schema,
                        ItemCreation {
                            start,
                            duration: None,
                            properties,
                        },
                    )
                    .ok_or(TimelineEditError::PlacementUnavailable)?;
                if !editor.active_document_mut().import_item_file(id, imported) {
                    return Err(TimelineEditError::PlacementUnavailable);
                }
                let item = editor.item(id).expect("imported item");
                if item.start() != start {
                    return Err(TimelineEditError::PlacementUnavailable);
                }
                start = item.end_exclusive(editor.frame_rate());
                ids.push(id);
            }
            editor.selection.set(ids.iter().copied().collect());
            Ok((ids, true))
        })?;
        for file in files {
            self.cache_media_file(&file);
        }
        Ok(ids)
    }

    /// Move items and trim their right edges together as one undoable edit.
    pub fn set_item_intervals(
        &mut self,
        intervals: Vec<ItemInterval>,
    ) -> Result<bool, TimelineEditError> {
        let mut edits = Vec::with_capacity(intervals.len());
        for interval in intervals {
            let mut candidate = self.active_document().prepare_item_edit(interval.item_id)?;
            let start = candidate.item.start();
            interval
                .start
                .get()
                .checked_add(interval.duration.get())
                .ok_or(TimelineEditError::PlacementUnavailable)?;
            if candidate.item.duration(self.frame_rate()) != interval.duration {
                candidate
                    .item
                    .resize_to(
                        start,
                        interval.duration,
                        ResizeMode::Trim,
                        self.frame_rate(),
                    )
                    .ok_or(TimelineEditError::PlacementUnavailable)?;
            }
            candidate.item.move_to(interval.start);
            edits.push(candidate);
        }
        self.commit_item_edits(None, edits)
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
        self.remove_items([id])
    }

    pub fn remove_selected_item(&mut self) -> bool {
        let selected = self.selected_item_ids().collect::<Vec<_>>();
        self.remove_items(selected)
    }

    /// Remove the requested items together as one undoable edit.
    pub fn remove_items(&mut self, ids: impl IntoIterator<Item = ItemId>) -> bool {
        let ids = ids
            .into_iter()
            .filter(|id| self.active_document().item(*id).is_some())
            .collect::<HashSet<_>>();
        if ids.is_empty() {
            return false;
        }
        self.edit_project_if_changed(EditImpact::MediaDependencies, None, |editor| {
            let mut changed = false;
            for id in &ids {
                changed |= editor.active_document_mut().remove_item(*id);
                editor.selection.remove(*id);
            }
            editor.remove_active_scene_bindings_for(&ids);
            changed
        })
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

        self.edit_project_option(EditImpact::MediaDependencies, None, |editor| {
            editor
                .insert_item_copies(
                    sources,
                    source_scene,
                    source_bindings,
                    (target_layer, target_start),
                    CopyPlacement::FindSpace,
                )
                .map(|copies| {
                    let mut ids = copies.into_values().collect::<Vec<_>>();
                    ids.sort_unstable_by_key(|id| id.get());
                    ids
                })
        })
    }

    fn insert_item_copies(
        &mut self,
        sources: &[(LayerId, TimelineItem)],
        source_scene: Option<SceneId>,
        source_bindings: &[(String, SceneBindingTarget)],
        (target_layer, target_start): (LayerId, Frame),
        placement: CopyPlacement,
    ) -> Option<HashMap<ItemId, ItemId>> {
        let active_scene = self.active_scene_id();
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

        let document = self.active_document_mut();
        let item_ids =
            document.insert_item_copies(&copies, target_layer, target_start, placement)?;
        let item_id_map = item_ids.into_iter().collect::<HashMap<_, _>>();

        if source_scene == Some(active_scene) {
            let scene = self.active_scene_mut();
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
            let scene = self.active_scene();
            for variable in scene.variables() {
                for binding in &variable.bindings {
                    let resolved = crate::timeline::resolve_scene_binding(
                        &self.project().scenes,
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

        self.next_effect_id = next_effect_id;
        self.selection.set(item_id_map.values().copied().collect());
        Some(item_id_map)
    }

    fn split_candidates(&self, at: Frame) -> impl Iterator<Item = (LayerId, &TimelineItem)> {
        self.selected_item_ids().filter_map(move |id| {
            let item = self.item(id)?;
            if item.start() >= at || at >= item.end_exclusive(self.frame_rate()) {
                return None;
            }
            Some((self.item_layer(id)?, item))
        })
    }

    pub fn can_split_selected_items(&self, at: Frame) -> bool {
        self.split_candidates(at).next().is_some()
    }

    /// Split selected intervals using the same trim behavior as edge resizing.
    pub fn split_selected_items(&mut self, at: Frame) -> bool {
        let sources = self
            .split_candidates(at)
            .map(|(layer, item)| (layer, item.clone()))
            .collect::<Vec<_>>();
        let Some(target_layer) = sources.iter().map(|(layer, _)| layer.get()).min() else {
            return false;
        };
        self.edit_project_option(EditImpact::MediaDependencies, None, |editor| {
            let rate = editor.frame_rate();
            let mut selected = editor.selected_item_ids().collect::<HashSet<_>>();
            let mut left_edits = Vec::with_capacity(sources.len());
            let mut right_items = Vec::with_capacity(sources.len());
            for (layer, source) in &sources {
                let mut left = ItemEdit::new(source.clone());
                left.item.resize_to(
                    source.start(),
                    FrameDuration::new(at.get() - source.start().get())?,
                    ResizeMode::Trim,
                    rate,
                )?;
                let mut right = source.clone();
                right.resize_to(
                    at,
                    FrameDuration::new(source.end_exclusive(rate).get() - at.get())?,
                    ResizeMode::Trim,
                    rate,
                )?;
                left_edits.push(left);
                right_items.push((*layer, right));
                selected.remove(&source.id);
            }
            let document = editor.active_document_mut();
            let edits = document.prepare_item_edits(left_edits).ok()?;
            document.publish_item_edits(edits);
            let copies =
                editor.duplicate_items_at(&right_items, (LayerId::new(target_layer), at))?;
            selected.extend(copies.into_values());
            editor.selection.set(selected);
            Some(())
        })
        .is_some()
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

    pub fn begin_item_move(&self, item_id: ItemId) -> Option<ItemMoveEdit> {
        let items = if self.is_item_selected(item_id) {
            self.selected_items()
        } else {
            vec![self.item(item_id)?.clone()]
        };
        let origins = items
            .into_iter()
            .map(|item| Some((self.item_layer(item.id)?, item)))
            .collect::<Option<Vec<_>>>()?;
        Some(ItemMoveEdit {
            origins,
            copies: None,
            session: self.begin_edit_gesture(),
        })
    }

    pub fn move_items(
        &mut self,
        edit: &mut ItemMoveEdit,
        frame_delta: i64,
        layer_delta: i64,
        duplicate: bool,
    ) -> bool {
        let (frame_delta, layer_delta) =
            edit.clamp_delta(frame_delta, layer_delta, self.frame_rate());
        let ItemMoveEdit {
            origins,
            copies,
            session,
        } = edit;
        self.update_edit_session(session, |editor| match (duplicate, copies.as_ref()) {
            (true, None) => {
                let Some(created) =
                    editor.edit_project_option(EditImpact::MediaDependencies, None, |editor| {
                        editor.start_item_copy_move(origins, frame_delta, layer_delta)
                    })
                else {
                    return false;
                };
                *copies = Some(created);
                true
            }
            (false, Some(created)) => {
                let changed = editor
                    .edit_project_option(EditImpact::MediaDependencies, None, |editor| {
                        editor.stop_item_copy_move(origins, created, frame_delta, layer_delta)
                    })
                    .is_some();
                if changed {
                    *copies = None;
                }
                changed
            }
            _ => {
                let positions = origins
                    .iter()
                    .map(|(layer, item)| {
                        let id = copies.as_ref().map_or(item.id, |copies| copies[&item.id]);
                        (id, item.start(), *layer)
                    })
                    .collect::<Vec<_>>();
                let Some(edits) = editor.active_document().prepare_move_items_from(
                    &positions,
                    frame_delta,
                    layer_delta,
                ) else {
                    return false;
                };
                let changed = editor.commit_item_edits(None, edits).unwrap_or(false);
                if changed {
                    editor
                        .selection
                        .set(positions.iter().map(|(id, _, _)| *id).collect());
                }
                changed
            }
        })
    }

    fn start_item_copy_move(
        &mut self,
        origins: &[(LayerId, TimelineItem)],
        frame_delta: i64,
        layer_delta: i64,
    ) -> Option<HashMap<ItemId, ItemId>> {
        let positions = origins
            .iter()
            .map(|(layer, item)| (item.id, item.start(), *layer))
            .collect::<Vec<_>>();
        let document = self.active_document_mut();
        let edits = document.prepare_move_items_from(&positions, 0, 0)?;
        let edits = document.prepare_item_edits(edits).ok()?;
        document.publish_item_edits(edits);

        let target_layer = origins
            .iter()
            .map(|(layer, _)| layer.get())
            .min()?
            .checked_add_signed(layer_delta)?;
        let target_start = origins
            .iter()
            .map(|(_, item)| item.start().get())
            .min()?
            .checked_add_signed(frame_delta)?;
        self.duplicate_items_at(
            origins,
            (LayerId::new(target_layer), Frame::new(target_start)),
        )
    }

    fn duplicate_items_at(
        &mut self,
        sources: &[(LayerId, TimelineItem)],
        target: (LayerId, Frame),
    ) -> Option<HashMap<ItemId, ItemId>> {
        let source_ids = sources
            .iter()
            .map(|(_, item)| item.id)
            .collect::<HashSet<_>>();
        let bindings = self
            .active_scene()
            .variables()
            .iter()
            .flat_map(|variable| {
                variable
                    .bindings
                    .iter()
                    .filter(|binding| source_ids.contains(&binding.item_id()))
                    .map(|binding| (variable.id().to_owned(), binding.clone()))
            })
            .collect::<Vec<_>>();
        self.insert_item_copies(
            sources,
            Some(self.active_scene_id()),
            &bindings,
            target,
            CopyPlacement::Exact,
        )
    }

    fn stop_item_copy_move(
        &mut self,
        origins: &[(LayerId, TimelineItem)],
        copies: &HashMap<ItemId, ItemId>,
        frame_delta: i64,
        layer_delta: i64,
    ) -> Option<()> {
        for id in copies.values() {
            if !self.active_document_mut().remove_item(*id) {
                return None;
            }
        }
        self.remove_active_scene_bindings_for(&copies.values().copied().collect());
        let positions = origins
            .iter()
            .map(|(layer, item)| (item.id, item.start(), *layer))
            .collect::<Vec<_>>();
        let document = self.active_document_mut();
        // Releasing Ctrl still removes copies if the requested move is blocked.
        if let Some(edits) = document
            .prepare_move_items_from(&positions, frame_delta, layer_delta)
            .and_then(|edits| document.prepare_item_edits(edits).ok())
        {
            document.publish_item_edits(edits);
        }
        self.selection
            .set(positions.iter().map(|(id, _, _)| *id).collect());
        Some(())
    }
}
