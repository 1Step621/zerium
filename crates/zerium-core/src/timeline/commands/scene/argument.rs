use crate::property::{PropertyConfiguration, PropertySchema, PropertyValue};
use crate::timeline::commands::SceneArgumentEditError;
use crate::timeline::history::HistoryKey;
use crate::timeline::scene::{
    apply_scene_binding_to_item, resolve_scene_binding, unique_scene_argument_name,
};
use crate::timeline::{
    EffectInstanceId, ItemId, SceneArgument, SceneArgumentPreset, SceneBindingOwner,
    SceneBindingTarget, SceneDefinition, SceneId, TimelineEditor, TimelineItem,
};
use std::collections::{HashMap, HashSet};

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

impl TimelineEditor {
    pub(in crate::timeline::commands) fn active_scene_has_binding(
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

    pub(in crate::timeline::commands) fn value_preserves_active_bindings(
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
            let argument = SceneArgument::new(schema, Vec::new())
                .expect("supported scene argument types must produce an argument schema");
            scene.arguments.push(argument);
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
        self.edit_project_if_changed(Some(key), |editor| {
            editor
                .project_mut()
                .scenes
                .get_mut(&scene_id)
                .and_then(|scene| scene.argument_mut(argument_id))
                .is_some_and(|argument| argument.set_numeric_settings(settings))
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
        self.try_edit_project(Some(key), |editor| {
            let argument = editor
                .project_mut()
                .scenes
                .get_mut(&scene_id)
                .and_then(|scene| scene.argument_mut(argument_id))
                .ok_or(SceneArgumentEditError::ArgumentNotFound)?;
            let changed = argument
                .set_default(&value)
                .ok_or(SceneArgumentEditError::IncompatibleContract)?;
            Ok((changed, changed))
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
