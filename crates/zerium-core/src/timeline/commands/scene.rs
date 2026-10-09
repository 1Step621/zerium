use std::collections::{HashMap, HashSet};

use crate::property::{PropertyConfiguration, PropertySchema, PropertyValue};
use crate::timeline::history::HistoryKey;
use crate::timeline::scene::{
    apply_scene_binding_to_item, resolve_scene_binding, unique_scene_argument_name,
};
use crate::timeline::{
    EffectInstanceId, Frame, FrameDuration, ItemId, LayerId, SceneArgument, SceneArgumentPreset,
    SceneBindingOwner, SceneBindingTarget, SceneDefinition, SceneId, TimelineDocument,
    TimelineEditor, TimelineItem,
};

use super::{SceneArgumentEditError, TimelineEditError};

pub(super) fn remove_bindings_for_items(scene: &mut SceneDefinition, item_ids: &HashSet<ItemId>) {
    for argument in &mut scene.arguments {
        argument
            .bindings
            .retain(|binding| !item_ids.contains(&binding.item_id()));
    }
}

fn remove_bindings_for_nested_argument(
    scenes: &mut HashMap<SceneId, SceneDefinition>,
    nested_scene_id: SceneId,
    argument_id: &str,
) {
    for scene in scenes.values_mut() {
        let item_ids = scene
            .items()
            .filter(|item| item.scene_id() == Some(nested_scene_id))
            .map(|item| item.id)
            .collect::<HashSet<_>>();
        for argument in &mut scene.arguments {
            argument.bindings.retain(|binding| {
                !(item_ids.contains(&binding.item_id())
                    && binding.owner() == SceneBindingOwner::Item
                    && binding.property_id() == argument_id)
            });
        }
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

// Scene definition and scene-argument commands.
impl TimelineEditor {
    pub(super) fn active_scene_has_binding(
        &self,
        item_id: ItemId,
        effect_id: Option<EffectInstanceId>,
        property_id: &str,
        predicate: impl Fn(&SceneBindingTarget) -> bool,
    ) -> bool {
        self.active_scene_id()
            .and_then(|scene_id| self.project().scenes.get(&scene_id))
            .is_some_and(|scene| {
                scene
                    .arguments
                    .iter()
                    .flat_map(|argument| &argument.bindings)
                    .any(|binding| {
                        binding.matches(item_id, effect_id, property_id) && predicate(binding)
                    })
            })
    }

    pub(super) fn value_preserves_active_bindings(
        &self,
        item_id: ItemId,
        effect_id: Option<EffectInstanceId>,
        property_id: &str,
        value: &PropertyValue,
    ) -> bool {
        !self.active_scene_has_binding(item_id, effect_id, property_id, |binding| {
            binding.element_id().is_some_and(|id| match value {
                PropertyValue::Array(values) => {
                    !values.iter().any(|element| element.element_id() == id)
                }
                _ => true,
            })
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
        let selected = self.selection.current.clone();
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
            let arguments = editor
                .active_scene_id()
                .and_then(|id| editor.scene(id))
                .into_iter()
                .flat_map(|scene| &scene.arguments)
                .filter_map(|argument| {
                    let bindings = argument
                        .bindings
                        .iter()
                        .filter(|binding| selected.contains(&binding.item_id()))
                        .cloned()
                        .collect::<Vec<_>>();
                    if bindings.is_empty() {
                        return None;
                    }
                    let mut schema = argument.schema.clone();
                    // Forward values unchanged across the new scene boundary. Each
                    // original target still applies its own constraints.
                    schema.configuration_mut(None).constraints = Default::default();
                    Some(SceneArgument::new(schema, bindings))
                })
                .collect();
            let scene = SceneDefinition::from_project(
                scene_id,
                name,
                arguments,
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
                for argument in &scene.arguments {
                    let parent_argument = parent
                        .argument_mut(argument.schema.id())
                        .expect("forwarded arguments came from the parent scene");
                    parent_argument
                        .bindings
                        .retain(|binding| !selected.contains(&binding.item_id()));
                    parent_argument.bindings.push(SceneBindingTarget::new(
                        instance,
                        SceneBindingOwner::Item,
                        argument.schema.id(),
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

    fn for_each_scene_instance_mut(
        &mut self,
        scene_id: SceneId,
        mut update: impl FnMut(&mut TimelineItem),
    ) {
        let project = self.project_mut();
        for item in project
            .document
            .items_mut()
            .chain(
                project
                    .scenes
                    .values_mut()
                    .flat_map(|scene| scene.document_mut().items_mut()),
            )
            .filter(|item| item.scene_id() == Some(scene_id))
        {
            update(item);
        }
    }

    fn apply_scene_binding(
        &mut self,
        scene_id: SceneId,
        target: &SceneBindingTarget,
        value: PropertyValue,
    ) -> Option<bool> {
        let schema = {
            let scene = self.project().scenes.get(&scene_id)?;
            let resolved = resolve_scene_binding(&self.project().scenes, scene, target)?;
            (!resolved.animated).then_some(resolved.schema)?
        };
        let item = self
            .project_mut()
            .scenes
            .get_mut(&scene_id)
            .and_then(|scene| scene.document_mut().item_mut(target.item_id()))?;
        apply_scene_binding_to_item(item, target, &schema, value)
    }

    pub fn create_scene_argument(
        &mut self,
        preset: SceneArgumentPreset,
        label: impl FnOnce(u64) -> String,
    ) -> Option<String> {
        let scene_id = self.active_scene_id()?;
        self.edit_project_option(None, |editor| {
            let scene = editor.project_mut().scenes.get_mut(&scene_id)?;
            let (argument_id, ordinal) = scene.allocate_argument_id()?;
            let label = unique_scene_argument_name(&scene.arguments, &label(ordinal), &argument_id);
            let default = match preset {
                SceneArgumentPreset::Number => PropertyValue::F32(0.),
                SceneArgumentPreset::SignedInteger => PropertyValue::I32(0),
                SceneArgumentPreset::UnsignedInteger => PropertyValue::U32(0),
                SceneArgumentPreset::Boolean => PropertyValue::Bool(false),
                SceneArgumentPreset::Color => PropertyValue::Color([0., 0., 0., 1.]),
                SceneArgumentPreset::Text => PropertyValue::String(String::new()),
                SceneArgumentPreset::File => PropertyValue::File(None),
            };
            let ty = preset.ty();
            let animatable = ty.is_interpolatable();
            let schema = PropertySchema::new_scalar(
                argument_id.clone(),
                label.into(),
                ty,
                default,
                PropertyConfiguration {
                    scene_bindable: true,
                    editable: true,
                    animatable,
                    ..Default::default()
                },
            );
            let schema = schema
                .for_scene_argument()
                .expect("supported scene argument types must produce an argument schema");
            scene.arguments.push(SceneArgument::new(schema, Vec::new()));
            Some(argument_id)
        })
    }

    pub fn rename_scene_argument(&mut self, argument_id: &str, label: &str) -> bool {
        let Some(scene_id) = self.active_scene_id() else {
            return false;
        };
        let label = label.trim();
        if label.is_empty() {
            return false;
        }
        let key = HistoryKey::SceneArgumentLabel(scene_id, argument_id.to_owned());
        self.edit_project_if_changed(Some(key), |editor| {
            let Some(scene) = editor.project().scenes.get(&scene_id) else {
                return false;
            };
            if scene.arguments.iter().any(|argument| {
                argument.schema.id() != argument_id && argument.schema.label() == label
            }) {
                return false;
            }
            let Some(argument) = scene.argument(argument_id) else {
                return false;
            };
            if argument.schema.label() == label {
                return false;
            }
            editor
                .project_mut()
                .scenes
                .get_mut(&scene_id)
                .and_then(|scene| scene.argument_mut(argument_id))
                .expect("the scene argument was checked")
                .schema
                .label = label.into();
            true
        })
    }

    pub fn move_scene_argument(&mut self, argument_id: &str, direction: i64) -> bool {
        if direction == 0 {
            return false;
        }
        let Some(scene_id) = self.active_scene_id() else {
            return false;
        };
        let Some(scene) = self.project().scenes.get(&scene_id) else {
            return false;
        };
        let Some(index) = scene
            .arguments
            .iter()
            .position(|argument| argument.schema.id() == argument_id)
        else {
            return false;
        };
        let Ok(direction) = isize::try_from(direction) else {
            return false;
        };
        let Some(target) = index.checked_add_signed(direction) else {
            return false;
        };
        if target >= scene.arguments.len() {
            return false;
        }

        self.edit_project_if_changed(None, |editor| {
            let scene = editor
                .project_mut()
                .scenes
                .get_mut(&scene_id)
                .expect("the active scene was checked");
            scene.arguments.swap(index, target);
            true
        })
    }

    pub fn update_scene_argument_numeric_settings(
        &mut self,
        argument_id: &str,
        settings: crate::property::NumericSettings,
    ) -> bool {
        let Some(scene_id) = self.active_scene_id() else {
            return false;
        };
        let key = HistoryKey::SceneArgumentSettings(scene_id, argument_id.to_owned());
        let Some(argument) = self
            .project()
            .scenes
            .get(&scene_id)
            .and_then(|scene| scene.argument(argument_id))
        else {
            return false;
        };
        let Some(next_schema) = argument.schema.with_scene_numeric_settings(settings) else {
            return false;
        };
        if next_schema == argument.schema {
            return false;
        }
        self.edit_project_if_changed(Some(key), |editor| {
            let argument = editor
                .project_mut()
                .scenes
                .get_mut(&scene_id)
                .and_then(|scene| scene.argument_mut(argument_id))
                .expect("the scene argument was checked above");
            argument.schema = next_schema;
            true
        })
    }

    pub fn update_scene_argument_default(
        &mut self,
        argument_id: &str,
        value: PropertyValue,
    ) -> Result<bool, SceneArgumentEditError> {
        let scene_id = self
            .active_scene_id()
            .ok_or(SceneArgumentEditError::NoActiveScene)?;
        let key = HistoryKey::SceneArgumentSettings(scene_id, argument_id.to_owned());
        let argument = self
            .project()
            .scenes
            .get(&scene_id)
            .and_then(|scene| scene.argument(argument_id))
            .ok_or(SceneArgumentEditError::ArgumentNotFound)?;
        let next_schema = argument
            .schema
            .with_scene_default(&value)
            .ok_or(SceneArgumentEditError::IncompatibleContract)?;
        if next_schema == argument.schema {
            return Ok(false);
        }
        self.try_edit_project(Some(key), |editor| {
            let argument = editor
                .project_mut()
                .scenes
                .get_mut(&scene_id)
                .and_then(|scene| scene.argument_mut(argument_id))
                .expect("the scene argument was checked above");
            argument.schema = next_schema;
            Ok((true, true))
        })
    }

    pub fn connect_scene_argument(
        &mut self,
        argument_id: &str,
        target: SceneBindingTarget,
    ) -> Result<(), SceneArgumentEditError> {
        let Some(scene_id) = self.active_scene_id() else {
            return Err(SceneArgumentEditError::NoActiveScene);
        };
        let Some(scene) = self.project().scenes.get(&scene_id) else {
            return Err(SceneArgumentEditError::NoActiveScene);
        };
        if scene
            .arguments
            .iter()
            .flat_map(|argument| &argument.bindings)
            .any(|binding| binding == &target)
        {
            return Err(SceneArgumentEditError::TargetAlreadyBound);
        }
        let Some(resolved) = resolve_scene_binding(&self.project().scenes, scene, &target) else {
            return Err(SceneArgumentEditError::TargetNotFound);
        };
        if resolved.animated {
            return Err(SceneArgumentEditError::TargetAnimated);
        }
        if !resolved.schema.is_scene_bindable(None) {
            return Err(SceneArgumentEditError::TargetNotBindable);
        }
        let Some(argument) = scene.argument(argument_id) else {
            return Err(SceneArgumentEditError::ArgumentNotFound);
        };
        if !resolved.schema.same_type(&argument.schema) {
            return Err(SceneArgumentEditError::IncompatibleContract);
        }
        self.try_edit_project(None, |editor| {
            let Some(argument) = editor
                .project_mut()
                .scenes
                .get_mut(&scene_id)
                .and_then(|scene| scene.argument_mut(argument_id))
            else {
                return Err(SceneArgumentEditError::ArgumentNotFound);
            };
            argument.bindings.push(target);
            Ok(((), true))
        })
    }

    pub fn disconnect_scene_argument(
        &mut self,
        argument_id: &str,
        target: &SceneBindingTarget,
    ) -> Result<(), SceneArgumentEditError> {
        let scene_id = self
            .active_scene_id()
            .ok_or(SceneArgumentEditError::NoActiveScene)?;
        let argument = self
            .scene(scene_id)
            .and_then(|scene| scene.argument(argument_id))
            .ok_or(SceneArgumentEditError::ArgumentNotFound)?;
        if !argument.bindings.contains(target) {
            return Err(SceneArgumentEditError::TargetNotFound);
        }
        let value = argument.schema.default_value();
        self.try_edit_project(None, |editor| {
            editor
                .apply_scene_binding(scene_id, target, value)
                .expect("validated binding must accept its value");
            editor
                .project_mut()
                .scenes
                .get_mut(&scene_id)
                .and_then(|scene| scene.argument_mut(argument_id))
                .expect("argument was checked")
                .bindings
                .retain(|binding| binding != target);
            Ok(((), true))
        })
    }

    pub fn remove_scene_argument(
        &mut self,
        argument_id: &str,
    ) -> Result<(), SceneArgumentEditError> {
        let Some(scene_id) = self.active_scene_id() else {
            return Err(SceneArgumentEditError::NoActiveScene);
        };
        let argument = self
            .scene(scene_id)
            .and_then(|scene| scene.argument(argument_id))
            .ok_or(SceneArgumentEditError::ArgumentNotFound)?;
        let bindings = argument.bindings.clone();
        let value = argument.schema.default_value();
        self.try_edit_project(None, |editor| {
            for target in &bindings {
                editor
                    .apply_scene_binding(scene_id, target, value.clone())
                    .expect("validated binding must accept its value");
            }
            editor
                .project_mut()
                .scenes
                .get_mut(&scene_id)
                .expect("active scene exists")
                .arguments
                .retain(|argument| argument.schema.id() != argument_id);
            editor.for_each_scene_instance_mut(scene_id, |item| {
                item.properties.remove(argument_id);
                item.animations.retain_valid_for_property(argument_id, None);
            });
            remove_bindings_for_nested_argument(
                &mut editor.project_mut().scenes,
                scene_id,
                argument_id,
            );
            Ok(((), true))
        })
    }
}
