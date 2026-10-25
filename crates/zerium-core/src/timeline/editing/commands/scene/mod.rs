//! Commands on scene definitions and placed instances.
mod calculation;
mod variable;

use crate::timeline::{
    Frame, FrameDuration, ItemId, LayerId, SceneBindingOwner, SceneBindingTarget, SceneDefinition,
    SceneId, TimelineDocument, TimelineEditError, TimelineEditor, TimelineItem,
    editing::history::{EditImpact, HistoryKey},
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
        if self.active_scene_id() != scene_id || scene_id == self.main_scene_id() {
            return false;
        }
        self.edit_project_if_changed(
            EditImpact::Values,
            Some(HistoryKey::SceneStretchPoint(scene_id)),
            |editor| {
                editor
                    .project_mut()
                    .scenes_mut()
                    .get_mut(&scene_id)
                    .is_some_and(|scene| scene.set_stretch_point(point))
            },
        )
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
        scene_id != self.main_scene_id()
            && self.project().scenes.contains_key(&scene_id)
            && !self.scene_reaches(scene_id, self.active_scene_id(), &mut HashSet::new())
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
        if scene_id == self.main_scene_id()
            || self.scene_reaches(scene_id, self.active_scene_id(), &mut HashSet::new())
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
            let mut start = first.start();
            let mut end = first.end_exclusive(self.frame_rate());
            let mut min_layer = first_layer;
            let mut target_layer = first_layer;
            for (layer, item) in selected_items {
                start = start.min(item.start());
                end = end.max(item.end_exclusive(self.frame_rate()));
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
        self.edit_project_option(EditImpact::MediaDependencies, None, |editor| {
            let frame_rate = editor.frame_rate();
            let mut items = editor.active_document_mut().take_items(&selected);
            for (layer, item) in &mut items {
                *layer = LayerId::new(layer.get().saturating_sub(min_layer.get()));
                item.move_to(Frame::new(item.start().get().saturating_sub(start.get())));
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
            let variables = editor.active_scene().variables_for_grouping(&selected);
            let scene = SceneDefinition::from_project(
                scene_id,
                name,
                variables,
                TimelineDocument::from_items(frame_rate, items),
            );
            let duration = FrameDuration::new(end.get().saturating_sub(start.get()))?;
            debug_assert_eq!(scene.duration(), duration);
            let instance = editor.active_document_mut().insert_item(
                target_layer,
                start,
                duration,
                |id, start, duration| TimelineItem::scene_instance(id, start, duration, scene_id),
            )?;

            {
                let parent = editor.active_scene_mut();
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
            editor.project_mut().scenes_mut().insert(scene_id, scene);
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
        self.edit_project_if_changed(EditImpact::Values, Some(key), |editor| {
            let Some(scene) = editor.project_mut().scenes_mut().get_mut(&scene_id) else {
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
        if scene_id == self.main_scene_id() || !self.project().scenes.contains_key(&scene_id) {
            return false;
        }
        self.edit_project_if_changed(EditImpact::MediaDependencies, None, |editor| {
            for scene in editor
                .project_mut()
                .scenes_mut()
                .values_mut()
                .filter(|scene| {
                    scene.id != scene_id
                        && scene.items().any(|item| item.scene_id() == Some(scene_id))
                })
            {
                let removed_items = remove_scene_instances(scene.document_mut(), scene_id);
                remove_bindings_for_items(scene, &removed_items);
            }
            editor.project_mut().scenes_mut().remove(&scene_id);

            if let Some(path_index) = editor.scene_path.iter().position(|id| *id == scene_id) {
                editor.scene_path.truncate(path_index);
                editor.reset_scene_session();
            }
            editor.selection.clear();
            editor.active_edit_target = None;
            editor.visibility.clear();
            true
        })
    }
}
