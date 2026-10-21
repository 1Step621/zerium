use crate::property::{PropertyConfiguration, PropertySchema, PropertyValue};
use crate::timeline::commands::SceneVariableEditError;
use crate::timeline::history::HistoryKey;
use crate::timeline::scene::{
    apply_scene_binding_to_item, resolve_scene_binding, unique_scene_variable_name,
};
use crate::timeline::{
    EffectInstanceId, ItemId, SceneBindingOwner, SceneBindingTarget, SceneDefinition, SceneId,
    SceneVariable, SceneVariablePreset, TimelineEditor, TimelineItem,
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
        for argument in scene.variables_mut() {
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
                    .variables()
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
        preset: SceneVariablePreset,
        label: impl FnOnce(u64) -> String,
    ) -> Option<String> {
        self.create_scene_variable(preset, label, None)
    }

    pub(super) fn create_scene_variable(
        &mut self,
        preset: SceneVariablePreset,
        label: impl FnOnce(u64) -> String,
        calculation: Option<crate::timeline::SceneCalculation>,
    ) -> Option<String> {
        let scene_id = self.active_scene_id()?;
        self.edit_project_option(None, |editor| {
            let scene = editor.project_mut().scenes.get_mut(&scene_id)?;
            let (argument_id, ordinal) = scene.allocate_variable_id()?;
            let label =
                unique_scene_variable_name(scene.variables(), &label(ordinal), &argument_id);
            let variable = if let Some(program) = calculation {
                SceneVariable::calculated(argument_id.clone(), label, preset.ty(), program)
            } else {
                let default = match preset {
                    SceneVariablePreset::Number => PropertyValue::F32(0.),
                    SceneVariablePreset::SignedInteger => PropertyValue::I32(0),
                    SceneVariablePreset::UnsignedInteger => PropertyValue::U32(0),
                    SceneVariablePreset::Boolean => PropertyValue::Bool(false),
                    SceneVariablePreset::Color => PropertyValue::Color([0., 0., 0., 1.]),
                    SceneVariablePreset::Text => PropertyValue::String(String::new()),
                    SceneVariablePreset::File => PropertyValue::File(None),
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
                SceneVariable::argument(schema, Vec::new()).expect("supported argument contract")
            };
            scene.variables_mut().push(variable);
            Some(argument_id)
        })
    }

    pub fn rename_scene_variable(&mut self, argument_id: &str, label: &str) -> bool {
        let Some(scene_id) = self.active_scene_id() else {
            return false;
        };
        let label = label.trim();
        if label.is_empty() {
            return false;
        }
        let key = HistoryKey::SceneVariableLabel(scene_id, argument_id.to_owned());
        self.edit_project_if_changed(Some(key), |editor| {
            let Some(scene) = editor.project().scenes.get(&scene_id) else {
                return false;
            };
            if scene
                .variables()
                .iter()
                .any(|argument| argument.id() != argument_id && argument.label() == label)
            {
                return false;
            }
            let Some(argument) = scene.variable(argument_id) else {
                return false;
            };
            if argument.label() == label {
                return false;
            }
            editor
                .project_mut()
                .scenes
                .get_mut(&scene_id)
                .and_then(|scene| scene.variable_mut(argument_id))
                .expect("the scene argument was checked")
                .rename(label.to_owned());
            true
        })
    }

    pub fn move_scene_variable(&mut self, argument_id: &str, direction: i64) -> bool {
        if direction == 0 {
            return false;
        }
        let Some(scene_id) = self.active_scene_id() else {
            return false;
        };
        let Some(scene) = self.project().scenes.get(&scene_id) else {
            return false;
        };
        let Some(variable) = scene.variable(argument_id) else {
            return false;
        };
        let indices = scene
            .variables()
            .iter()
            .enumerate()
            .filter(|(_, other)| other.calculation().is_some() == variable.calculation().is_some())
            .map(|(index, _)| index)
            .collect::<Vec<_>>();
        let Some(index) = indices
            .iter()
            .position(|&index| scene.variables()[index].id() == argument_id)
        else {
            return false;
        };
        let Ok(direction) = isize::try_from(direction) else {
            return false;
        };
        let Some(target) = index
            .checked_add_signed(direction)
            .and_then(|index| indices.get(index))
            .copied()
        else {
            return false;
        };
        let index = indices[index];

        self.edit_project_if_changed(None, |editor| {
            let scene = editor
                .project_mut()
                .scenes
                .get_mut(&scene_id)
                .expect("the active scene was checked");
            scene.variables_mut().swap(index, target);
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
                .and_then(|scene| scene.variable_mut(argument_id))
                .filter(|variable| variable.calculation().is_none())
                .is_some_and(|argument| argument.set_numeric_settings(settings))
        })
    }

    pub fn update_scene_argument_default(
        &mut self,
        argument_id: &str,
        value: PropertyValue,
    ) -> Result<bool, SceneVariableEditError> {
        let scene_id = self
            .active_scene_id()
            .ok_or(SceneVariableEditError::NoActiveScene)?;
        let key = HistoryKey::SceneArgumentSettings(scene_id, argument_id.to_owned());
        self.try_edit_project(Some(key), |editor| {
            let argument = editor
                .project_mut()
                .scenes
                .get_mut(&scene_id)
                .and_then(|scene| scene.variable_mut(argument_id))
                .filter(|variable| variable.calculation().is_none())
                .ok_or(SceneVariableEditError::VariableNotFound)?;
            let changed = argument
                .set_default(&value)
                .ok_or(SceneVariableEditError::IncompatibleContract)?;
            Ok((changed, changed))
        })
    }

    pub fn connect_scene_variable(
        &mut self,
        argument_id: &str,
        target: SceneBindingTarget,
    ) -> Result<(), SceneVariableEditError> {
        let Some(scene_id) = self.active_scene_id() else {
            return Err(SceneVariableEditError::NoActiveScene);
        };
        let Some(scene) = self.project().scenes.get(&scene_id) else {
            return Err(SceneVariableEditError::NoActiveScene);
        };
        if scene
            .variables()
            .iter()
            .flat_map(|argument| &argument.bindings)
            .any(|binding| binding == &target)
        {
            return Err(SceneVariableEditError::TargetAlreadyBound);
        }
        let Some(resolved) = resolve_scene_binding(&self.project().scenes, scene, &target) else {
            return Err(SceneVariableEditError::TargetNotFound);
        };
        if resolved.animated {
            return Err(SceneVariableEditError::TargetAnimated);
        }
        if !resolved.schema.is_scene_bindable(None) {
            return Err(SceneVariableEditError::TargetNotBindable);
        }
        let Some(argument) = scene.variable(argument_id) else {
            return Err(SceneVariableEditError::VariableNotFound);
        };
        if !resolved
            .schema
            .scalar_type(None, None)
            .is_some_and(|ty| ty.same_type(argument.ty()))
        {
            return Err(SceneVariableEditError::IncompatibleContract);
        }
        self.try_edit_project(None, |editor| {
            let Some(argument) = editor
                .project_mut()
                .scenes
                .get_mut(&scene_id)
                .and_then(|scene| scene.variable_mut(argument_id))
            else {
                return Err(SceneVariableEditError::VariableNotFound);
            };
            argument.bindings.push(target);
            Ok(((), true))
        })
    }

    pub fn disconnect_scene_variable(
        &mut self,
        argument_id: &str,
        target: &SceneBindingTarget,
    ) -> Result<(), SceneVariableEditError> {
        let scene_id = self
            .active_scene_id()
            .ok_or(SceneVariableEditError::NoActiveScene)?;
        let argument = self
            .scene(scene_id)
            .and_then(|scene| scene.variable(argument_id))
            .ok_or(SceneVariableEditError::VariableNotFound)?;
        if !argument.bindings.contains(target) {
            return Err(SceneVariableEditError::TargetNotFound);
        }
        let value = self
            .property_value(&crate::timeline::PropertyAddress {
                item_id: target.item_id(),
                effect_id: target.owner().effect_id(),
                property_id: target.property_id().to_owned(),
                element_id: target.element_id(),
                scalar_index: target.scalar_index(),
            })
            .ok_or(SceneVariableEditError::TargetNotFound)?;
        self.try_edit_project(None, |editor| {
            editor
                .apply_scene_binding(scene_id, target, value)
                .expect("validated binding must accept its value");
            editor
                .project_mut()
                .scenes
                .get_mut(&scene_id)
                .and_then(|scene| scene.variable_mut(argument_id))
                .expect("argument was checked")
                .bindings
                .retain(|binding| binding != target);
            Ok(((), true))
        })
    }

    pub fn remove_scene_variable(
        &mut self,
        argument_id: &str,
    ) -> Result<(), SceneVariableEditError> {
        let Some(scene_id) = self.active_scene_id() else {
            return Err(SceneVariableEditError::NoActiveScene);
        };
        let argument = self
            .scene(scene_id)
            .and_then(|scene| scene.variable(argument_id))
            .ok_or(SceneVariableEditError::VariableNotFound)?;
        if self.scene(scene_id).is_some_and(|scene| {
            scene.variables().iter().any(|variable| {
                variable.calculation().is_some_and(|calculation| {
                    calculation
                        .inputs
                        .iter()
                        .any(|input| input.variable_id == argument_id)
                })
            })
        }) {
            return Err(SceneVariableEditError::VariableInUse);
        }
        let is_argument = argument.argument_schema().is_some();
        let bindings = argument
            .bindings
            .iter()
            .map(|target| {
                let value = self
                    .property_value(&crate::timeline::PropertyAddress {
                        item_id: target.item_id(),
                        effect_id: target.owner().effect_id(),
                        property_id: target.property_id().to_owned(),
                        element_id: target.element_id(),
                        scalar_index: target.scalar_index(),
                    })
                    .ok_or(SceneVariableEditError::TargetNotFound)?;
                Ok((target.clone(), value))
            })
            .collect::<Result<Vec<_>, SceneVariableEditError>>()?;
        self.try_edit_project(None, |editor| {
            for (target, value) in bindings {
                editor
                    .apply_scene_binding(scene_id, &target, value)
                    .expect("validated binding must accept its resolved value");
            }
            editor
                .project_mut()
                .scenes
                .get_mut(&scene_id)
                .expect("active scene exists")
                .variables_mut()
                .retain(|argument| argument.id() != argument_id);
            if is_argument {
                editor.for_each_scene_instance_mut(scene_id, |item| {
                    item.properties.remove(argument_id);
                    item.animations.retain_valid_for_property(argument_id, None);
                });
                remove_bindings_for_nested_argument(
                    &mut editor.project_mut().scenes,
                    scene_id,
                    argument_id,
                );
            }
            Ok(((), true))
        })
    }
}
