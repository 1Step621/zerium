//! Commands on scene definitions and placed instances.
mod calculation;
mod variable;

use crate::timeline::commands::TimelineEditError;
use crate::timeline::history::HistoryKey;
use crate::timeline::{
    Frame, FrameDuration, ItemId, LayerId, SceneBindingOwner, SceneBindingTarget, SceneDefinition,
    SceneId, TimelineDocument, TimelineEditor, TimelineItem,
};
use std::collections::HashSet;

pub(super) fn remove_bindings_for_items(scene: &mut SceneDefinition, item_ids: &HashSet<ItemId>) {
    for argument in scene.variables_mut() {
        argument
            .bindings
            .retain(|binding| !item_ids.contains(&binding.item_id()));
    }
}

fn remove_scene_instances(document: &mut TimelineDocument, scene_id: SceneId) -> HashSet<ItemId> {
    let instance_ids = document
        .items()
        .filter(|item| item.scene_id() == Some(scene_id))
        .map(|item| item.id)
        .collect::<HashSet<_>>();
    for item_id in &instance_ids {
        let removed = document.remove_item(*item_id);
        debug_assert!(removed, "collected scene instance must still exist");
    }
    instance_ids
}

impl TimelineEditor {
    pub fn set_scene_stretch_point(&mut self, scene_id: SceneId, point: Frame) -> bool {
        if self.active_scene_id() != Some(scene_id) {
            return false;
        }
        self.edit_project_if_changed(Some(HistoryKey::SceneStretchPoint(scene_id)), |editor| {
            editor
                .project_mut()
                .scenes
                .get_mut(&scene_id)
                .is_some_and(|scene| scene.set_stretch_point(point))
        })
    }

    fn scene_reaches(
        &self,
        from: SceneId,
        target: SceneId,
        visited: &mut HashSet<SceneId>,
    ) -> bool {
        if from == target {
            return true;
        }
        if !visited.insert(from) {
            return false;
        }
        self.project().scenes.get(&from).is_some_and(|scene| {
            scene
                .document()
                .items()
                .filter_map(TimelineItem::scene_id)
                .any(|child| self.scene_reaches(child, target, visited))
        })
    }

    pub fn can_add_scene_instance(&self, scene_id: SceneId) -> bool {
        self.project().scenes.contains_key(&scene_id)
            && self
                .active_scene_id()
                .is_none_or(|active| !self.scene_reaches(scene_id, active, &mut HashSet::new()))
    }

    pub fn add_scene_instance(
        &mut self,
        layer: LayerId,
        start: Frame,
        scene_id: SceneId,
    ) -> Result<ItemId, TimelineEditError> {
        let duration = self
            .scene(scene_id)
            .ok_or(TimelineEditError::SceneNotFound(scene_id))?
            .duration();
        if self
            .active_scene_id()
            .is_some_and(|active| self.scene_reaches(scene_id, active, &mut HashSet::new()))
        {
            return Err(TimelineEditError::RecursiveSceneReference);
        }
        self.edit_item_creation(|editor| {
            let id = editor
                .active_document_mut()
                .insert_item(layer, start, duration, move |id, start, duration| {
                    TimelineItem::scene_instance(id, start, duration, scene_id)
                })
                .ok_or(TimelineEditError::PlacementUnavailable)?;
            editor.selection.select_only(id);
            Ok(id)
        })
    }

    pub fn group_selected_as_scene(&mut self, base_name: &str) -> Option<SceneId> {
        let selected = self.selected_item_ids().collect::<HashSet<_>>();
        if selected.is_empty() {
            return None;
        }
        let (start, end, min_layer, target_layer) = {
            let document = self.active_document();
            let mut selected_items = selected
                .iter()
                .filter_map(|id| Some((document.item_layer(*id)?, document.item(*id)?)));
            let (first_layer, first) = selected_items.next()?;
            let mut start = first.start;
            let mut end = first.end_exclusive();
            let mut min_layer = first_layer;
            let mut target_layer = first_layer;
            for (layer, item) in selected_items {
                start = start.min(item.start);
                end = end.max(item.end_exclusive());
                min_layer = LayerId::new(min_layer.get().min(layer.get()));
                target_layer = LayerId::new(target_layer.get().max(layer.get()));
            }
            let collides =
                document.overlaps_on_layer_excluding(target_layer, start, end, &selected);
            if collides {
                return None;
            }
            (start, end, min_layer, target_layer)
        };

        let raw_scene_id = self.next_scene_id?;
        self.edit_project_option(None, |editor| {
            let frame_rate = editor.frame_rate();
            let mut next_document = editor.active_document().clone();
            let mut items = next_document.take_items(&selected);
            for (layer, item) in &mut items {
                *layer = LayerId::new(layer.get().saturating_sub(min_layer.get()));
                item.start = Frame::new(item.start.get().saturating_sub(start.get()));
            }
            let scene_id = SceneId::new(editor.project().id, raw_scene_id);
            let name = if editor
                .project()
                .scenes
                .values()
                .all(|scene| scene.name != base_name)
            {
                base_name.to_owned()
            } else {
                (2_u64..)
                    .map(|index| format!("{base_name} {index}"))
                    .find(|name| {
                        editor
                            .project()
                            .scenes
                            .values()
                            .all(|scene| scene.name != *name)
                    })
                    .expect("a scene name suffix must eventually be available")
            };
            let (variables, moved_calculations) = editor
                .active_scene_id()
                .and_then(|id| editor.scene(id))
                .map(|scene| scene.variables_for_grouping(&selected))
                .unwrap_or_default();
            let scene = SceneDefinition::from_project(
                scene_id,
                name,
                variables,
                TimelineDocument::from_items(frame_rate, items),
            );
            let duration = FrameDuration::new(end.get().saturating_sub(start.get()))?;
            debug_assert_eq!(scene.duration(), duration);
            let instance = next_document.insert_item(
                target_layer,
                start,
                duration,
                |id, start, duration| TimelineItem::scene_instance(id, start, duration, scene_id),
            )?;

            *editor.active_document_mut() = next_document;
            if let Some(parent) = editor
                .active_scene_id()
                .and_then(|id| editor.project_mut().scenes.get_mut(&id))
            {
                parent
                    .variables_mut()
                    .retain(|variable| !moved_calculations.contains(variable.id()));
                for variable in parent.variables_mut() {
                    variable
                        .bindings
                        .retain(|binding| !selected.contains(&binding.item_id()));
                }
                for argument in scene.arguments() {
                    parent
                        .variable_mut(argument.id())
                        .expect("forwarded argument exists in parent")
                        .bindings
                        .push(SceneBindingTarget::new(
                            instance,
                            SceneBindingOwner::Item,
                            argument.id(),
                            None,
                            None,
                        ));
                }
            }
            editor.project_mut().scenes.insert(scene_id, scene);
            editor.next_scene_id = raw_scene_id.checked_add(1).filter(|id| *id != u64::MAX);
            editor.selection.set(HashSet::from([instance]));
            Some(scene_id)
        })
    }

    pub fn rename_scene(&mut self, scene_id: SceneId, name: impl Into<String>) -> bool {
        let name = name.into();
        let name = name.trim();
        if name.is_empty()
            || self
                .project()
                .scenes
                .values()
                .any(|scene| scene.id != scene_id && scene.name == name)
        {
            return false;
        }
        let key = HistoryKey::SceneName(scene_id);
        self.edit_project_if_changed(Some(key), |editor| {
            let Some(scene) = editor.project_mut().scenes.get_mut(&scene_id) else {
                return false;
            };
            if scene.name == name {
                return false;
            }
            scene.name = name.to_owned();
            true
        })
    }

    pub fn delete_scene(&mut self, scene_id: SceneId) -> bool {
        if !self.project().scenes.contains_key(&scene_id) {
            return false;
        }
        self.edit_project_if_changed(None, |editor| {
            remove_scene_instances(&mut editor.project_mut().document, scene_id);
            for scene in editor
                .project_mut()
                .scenes
                .values_mut()
                .filter(|scene| scene.id != scene_id)
            {
                let removed_items = remove_scene_instances(scene.document_mut(), scene_id);
                remove_bindings_for_items(scene, &removed_items);
            }
            editor.project_mut().scenes.remove(&scene_id);

            if let Some(path_index) = editor.scene_path.iter().position(|id| *id == scene_id) {
                editor.scene_path.truncate(path_index);
                editor.playhead = Frame::new(0);
            }
            editor.selection.clear();
            editor.visibility.clear();
            true
        })
    }
}
