//! All public mutations of [`TimelineEditor`].
//!
//! This sibling module keeps the read-oriented editor API compact. The editor
//! internals it needs are visible only inside `timeline`.

use crate::domain::property::PropertyValueType;
use std::collections::{HashMap, HashSet};
use thiserror::Error;

use crate::domain::animation::{
    BezierHandle, ScalarAnimationAddress, ScalarAnimations, ScalarTrack, SegmentInterpolation,
};
use crate::domain::media::ImportedMedia;
use crate::domain::property::{
    PropertyElementId, PropertyScalarSchema, PropertySchema, PropertyType, PropertyValue,
    ScalarPropertyType,
};

use super::{
    document::{ResizeEdge, TimelineDocument},
    editor::{HistoryKey, HistorySnapshot, TimelineEditor},
    evaluation::evaluate_expression_arguments,
    expression,
    ids::{EffectInstanceId, ItemId, LayerId, SceneId},
    item::TimelineItem,
    scene::{
        SceneArgument, SceneArgumentPreset, SceneArgumentSchema, SceneBindingOwner,
        SceneBindingTarget, SceneDefinition, apply_scene_binding_to_item,
        materialize_scene_instance_properties, resolve_property_schema, resolve_scene_binding,
        scene_argument_expressions_valid, set_scene_instance_override, unique_scene_argument_name,
    },
    settings::ProjectResolution,
    time::{Frame, FrameDuration, FrameRate},
};

fn remove_bindings_for_items(scene: &mut SceneDefinition, item_ids: &HashSet<ItemId>) {
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
    let parents = scenes
        .iter()
        .map(|(scene_id, scene)| {
            let item_ids = scene
                .items()
                .filter(|item| item.scene_id() == Some(nested_scene_id))
                .map(|item| item.id)
                .collect::<HashSet<_>>();
            (*scene_id, item_ids)
        })
        .collect::<Vec<_>>();
    for (scene_id, item_ids) in parents {
        let Some(scene) = scenes.get_mut(&scene_id) else {
            continue;
        };
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SceneArgumentEditError {
    NoActiveScene,
    ArgumentNotFound,
    TargetNotFound,
    TargetAlreadyBound,
    TargetAnimated,
    TargetNotBindable,
    IncompatibleContract,
    ReferencedByExpression,
}

/// An expected reason why an editor command could not be applied.
///
/// Commands that merely report whether they changed state still return
/// `bool`; commands that can reject valid-looking user input use this type so
/// the UI does not have to guess why they failed.
#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub(crate) enum TimelineEditError {
    #[error("アイテム '{plugin_id}/{item_id}' がありません")]
    PluginItemNotFound { plugin_id: String, item_id: String },
    #[error("エフェクト '{plugin_id}/{effect_id}' がありません")]
    PluginEffectNotFound {
        plugin_id: String,
        effect_id: String,
    },
    #[error("シーン {} がありません", .0.get())]
    SceneNotFound(SceneId),
    #[error("シーン参照が循環します")]
    RecursiveSceneReference,
    #[error("対象のアイテムが選択されていません")]
    NothingSelected,
    #[error("新しいIDを割り当てられません")]
    IdentifierExhausted,
    #[error("指定した位置にアイテムを配置できません")]
    PlacementUnavailable,
    #[error("アイテム {} がありません", .0.get())]
    ItemNotFound(ItemId),
    #[error("ファイルの種類がアイテム入力と一致しません")]
    IncompatibleMedia,
}

// Scene definition and scene-argument commands.
impl TimelineEditor {
    fn active_scene_has_binding(
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

    fn value_preserves_active_bindings(
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

    fn scene_instance_property_schema(
        &self,
        item_id: ItemId,
        property_id: &str,
    ) -> Option<PropertySchema> {
        let item = self.active_document().item(item_id)?;
        item.scene_id()?;
        resolve_property_schema(
            &self.project().scenes,
            item,
            SceneBindingOwner::Item,
            property_id,
        )
        .cloned()
    }

    fn animation_schema(
        &self,
        item_id: ItemId,
        effect_id: Option<EffectInstanceId>,
        property_id: &str,
    ) -> Option<PropertySchema> {
        let item = self.active_document().item(item_id)?;
        let owner = effect_id.map_or(SceneBindingOwner::Item, SceneBindingOwner::Effect);
        resolve_property_schema(&self.project().scenes, item, owner, property_id).cloned()
    }

    fn animation_store_mut(
        &mut self,
        item_id: ItemId,
        effect_id: Option<EffectInstanceId>,
    ) -> Option<&mut ScalarAnimations> {
        let item = self.active_document_mut().item_mut(item_id)?;
        match effect_id {
            Some(effect_id) => item
                .effects
                .iter_mut()
                .find(|effect| effect.id == effect_id)
                .map(|effect| &mut effect.animations),
            None => Some(&mut item.animations),
        }
    }

    fn animation_track(
        &self,
        item_id: ItemId,
        effect_id: Option<EffectInstanceId>,
        address: &ScalarAnimationAddress,
    ) -> Option<&ScalarTrack> {
        self.active_document().item(item_id)?.animation_track(
            effect_id,
            address.property_id(),
            address.element_id(),
            address.scalar_index(),
        )
    }

    fn animation_track_mut(
        &mut self,
        item_id: ItemId,
        effect_id: Option<EffectInstanceId>,
        address: &ScalarAnimationAddress,
    ) -> Option<&mut ScalarTrack> {
        self.animation_store_mut(item_id, effect_id)?
            .track_mut(address)
    }

    fn edit_selected_animation(
        &mut self,
        item_id: ItemId,
        effect_id: Option<EffectInstanceId>,
        address: ScalarAnimationAddress,
        before: Option<HistorySnapshot>,
        key: Option<HistoryKey>,
        edit: impl FnOnce(&mut ScalarTrack) -> bool,
    ) -> bool {
        let changed = self
            .animation_schema(item_id, effect_id, address.property_id())
            .is_some_and(|schema| {
                schema.is_editable(address.scalar_index())
                    && self
                        .animation_track_mut(item_id, effect_id, &address)
                        .is_some_and(edit)
            });
        self.finish_project_edit_if_changed(changed, before, key)
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

    pub(crate) fn can_add_scene_instance(&self, scene_id: SceneId) -> bool {
        self.project().scenes.contains_key(&scene_id)
            && self
                .active_scene_id()
                .is_none_or(|active| !self.scene_reaches(scene_id, active, &mut HashSet::new()))
    }

    pub(crate) fn add_scene_instance(
        &mut self,
        layer: LayerId,
        start: Frame,
        scene_id: SceneId,
    ) -> Result<ItemId, TimelineEditError> {
        let scene = self
            .project()
            .scenes
            .get(&scene_id)
            .ok_or(TimelineEditError::SceneNotFound(scene_id))?
            .clone();
        if self
            .active_scene_id()
            .is_some_and(|active| self.scene_reaches(scene_id, active, &mut HashSet::new()))
        {
            return Err(TimelineEditError::RecursiveSceneReference);
        }
        let before = self.history_snapshot();
        let duration = scene.duration();
        let id = self
            .active_document_mut()
            .add_generated_item(layer, start, duration, move |id, start, duration| {
                scene.instantiate(id, start, duration)
            })
            .ok_or(TimelineEditError::PlacementUnavailable)?;
        self.selection.select_only(id);
        self.finish_project_edit(Some(before), Some(HistoryKey::ItemCreation(id)));
        Ok(id)
    }

    pub(crate) fn group_selected_as_scene(&mut self) -> Option<SceneId> {
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
        let before = self.history_snapshot();
        let frame_rate = self.frame_rate();
        let mut next_document = self.active_document().clone();
        let mut items = next_document.take_items(&selected);
        for (layer, item) in &mut items {
            *layer = LayerId::new(layer.get().saturating_sub(min_layer.get()));
            item.start = Frame::new(item.start.get().saturating_sub(start.get()));
        }
        let scene_id = SceneId::new(self.project().id, raw_scene_id);
        let base_name = "シーン";
        let name = if self
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
                    self.project()
                        .scenes
                        .values()
                        .all(|scene| scene.name != *name)
                })
                .expect("a scene name suffix must eventually be available")
        };
        let scene = SceneDefinition::new(scene_id, name, frame_rate, items);
        let duration = FrameDuration::new(end.get().saturating_sub(start.get()))?;
        debug_assert_eq!(scene.duration(), duration);
        let instance = next_document.add_generated_item(
            target_layer,
            start,
            duration,
            |id, start, duration| scene.instantiate(id, start, duration),
        )?;

        *self.active_document_mut() = next_document;
        self.project_mut().scenes.insert(scene_id, scene);
        self.next_scene_id = raw_scene_id.checked_add(1).filter(|id| *id != u64::MAX);
        self.selection.set(HashSet::from([instance]));
        self.finish_project_edit(Some(before), None);
        Some(scene_id)
    }

    pub(crate) fn rename_scene(&mut self, scene_id: SceneId, name: impl Into<String>) -> bool {
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
        let before = self.history_snapshot_for_edit(Some(&key));
        let Some(scene) = self.project_mut().scenes.get_mut(&scene_id) else {
            return false;
        };
        if scene.name == name {
            return false;
        }
        scene.name = name.to_owned();
        self.finish_project_edit(before, Some(key));
        true
    }

    pub(crate) fn delete_scene(&mut self, scene_id: SceneId) -> bool {
        if !self.project().scenes.contains_key(&scene_id) {
            return false;
        }
        let before = self.history_snapshot();

        remove_scene_instances(&mut self.project_mut().document, scene_id);
        for scene in self
            .project_mut()
            .scenes
            .values_mut()
            .filter(|scene| scene.id != scene_id)
        {
            let removed_items = remove_scene_instances(scene.document_mut(), scene_id);
            remove_bindings_for_items(scene, &removed_items);
        }
        self.project_mut().scenes.remove(&scene_id);
        self.clamp_all_scene_instances();

        if let Some(path_index) = self.scene_path.iter().position(|id| *id == scene_id) {
            self.scene_path.truncate(path_index);
            self.playhead = Frame::new(0);
        }
        self.selection.clear();
        self.visibility.clear();
        self.finish_project_edit(Some(before), None);
        true
    }

    fn for_each_scene_instance_mut(
        &mut self,
        scene_id: SceneId,
        mut update: impl FnMut(&mut TimelineItem),
    ) {
        for item in self.project_mut().document.items_mut() {
            if item.scene_id() == Some(scene_id) {
                update(item);
            }
        }
        for scene in self.project_mut().scenes.values_mut() {
            for item in scene.document_mut().items_mut() {
                if item.scene_id() == Some(scene_id) {
                    update(item);
                }
            }
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

    fn sync_computed_scene_binding_values(&mut self, scene_id: SceneId) {
        let updates = {
            let Some(scene) = self.project().scenes.get(&scene_id) else {
                return;
            };
            let mut numeric_values = scene
                .input_arguments()
                .filter(|argument| {
                    argument.schema.ty()
                        == &PropertyType::Value(PropertyValueType::Scalar(ScalarPropertyType::F32))
                })
                .filter_map(|argument| match argument.schema.default_value() {
                    PropertyValue::F32(value) => Some((argument.schema.id().to_owned(), *value)),
                    _ => None,
                })
                .collect::<HashMap<_, _>>();
            let mut values = HashMap::new();
            evaluate_expression_arguments(scene, &mut numeric_values, &mut values);
            scene
                .computed_arguments()
                .filter_map(|argument| {
                    let value = argument
                        .schema
                        .constrained_value(values.get(argument.schema.id())?)?;
                    Some(
                        argument
                            .bindings
                            .iter()
                            .cloned()
                            .map(move |binding| (binding, value.clone())),
                    )
                })
                .flatten()
                .collect::<Vec<_>>()
        };
        for (binding, value) in updates {
            self.apply_scene_binding(scene_id, &binding, value);
        }
    }

    pub(crate) fn add_scene_argument(
        &mut self,
        target: SceneBindingTarget,
    ) -> Result<String, SceneArgumentEditError> {
        let scene_id = self
            .active_scene_id()
            .ok_or(SceneArgumentEditError::NoActiveScene)?;
        let scene = self
            .project()
            .scenes
            .get(&scene_id)
            .ok_or(SceneArgumentEditError::NoActiveScene)?;
        if scene
            .arguments
            .iter()
            .flat_map(|argument| &argument.bindings)
            .any(|binding| binding == &target)
        {
            return Err(SceneArgumentEditError::TargetAlreadyBound);
        }
        let resolved = resolve_scene_binding(&self.project().scenes, scene, &target)
            .ok_or(SceneArgumentEditError::TargetNotFound)?;
        let item = scene
            .document()
            .item(target.item_id())
            .ok_or(SceneArgumentEditError::TargetNotFound)?;
        if target.conflicts_with_aspect_ratio_lock(item, item.aspect_ratio_locked) {
            return Err(SceneArgumentEditError::IncompatibleContract);
        }
        if resolved.animated {
            return Err(SceneArgumentEditError::TargetAnimated);
        }
        if !resolved.schema.scene_bindable {
            return Err(SceneArgumentEditError::TargetNotBindable);
        }
        let before = self.history_snapshot();
        let scene = self
            .project_mut()
            .scenes
            .get_mut(&scene_id)
            .ok_or(SceneArgumentEditError::NoActiveScene)?;
        let (argument_id, _) = scene.allocate_argument_id();
        let label =
            unique_scene_argument_name(&scene.arguments, resolved.schema.label(), &argument_id);
        let schema = SceneArgumentSchema::from_property(resolved.schema)
            .ok_or(SceneArgumentEditError::IncompatibleContract)?
            .with_identity(argument_id.clone(), label);
        scene
            .arguments
            .push(SceneArgument::input(schema, vec![target]));
        self.finish_project_edit(Some(before), None);
        Ok(argument_id)
    }

    pub(crate) fn create_scene_argument(&mut self, preset: SceneArgumentPreset) -> Option<String> {
        let scene_id = self.active_scene_id()?;
        let before = self.history_snapshot();
        let scene = self.project_mut().scenes.get_mut(&scene_id)?;
        let (argument_id, ordinal) = scene.allocate_argument_id();
        let label =
            unique_scene_argument_name(&scene.arguments, &format!("引数{ordinal}"), &argument_id);
        let default = match preset {
            SceneArgumentPreset::Number => PropertyValue::F32(0.),
            SceneArgumentPreset::SignedInteger => PropertyValue::I32(0),
            SceneArgumentPreset::UnsignedInteger => PropertyValue::U32(0),
            SceneArgumentPreset::Boolean => PropertyValue::Bool(false),
            SceneArgumentPreset::Color => PropertyValue::Color([0., 0., 0., 1.]),
            SceneArgumentPreset::Text => PropertyValue::String(String::new()),
        };
        let schema = PropertySchema {
            id: argument_id.clone(),
            label,
            ty: PropertyType::Value(PropertyValueType::Scalar(preset.scalar())),
            default,
            scalars: vec![PropertyScalarSchema {
                editable: true,
                animatable: preset.scalar().is_interpolatable(),
                ..Default::default()
            }],
            scene_bindable: true,
        };
        let schema = SceneArgumentSchema::from_property(schema)
            .expect("supported scene argument types must produce a scalar schema");
        scene
            .arguments
            .push(SceneArgument::input(schema, Vec::new()));
        self.finish_project_edit(Some(before), None);
        Some(argument_id)
    }

    pub(crate) fn create_expression_scene_argument(&mut self) -> Option<String> {
        let scene_id = self.active_scene_id()?;
        let before = self.history_snapshot();
        let scene = self.project_mut().scenes.get_mut(&scene_id)?;
        let (argument_id, ordinal) = scene.allocate_argument_id();
        let label =
            unique_scene_argument_name(&scene.arguments, &format!("導出{ordinal}"), &argument_id);
        let expression = scene
            .input_arguments()
            .find(|argument| {
                argument.schema.ty()
                    == &PropertyType::Value(PropertyValueType::Scalar(ScalarPropertyType::F32))
            })
            .map(|argument| argument.schema.id().to_owned())
            .unwrap_or_else(|| "0".to_owned());
        let schema = PropertySchema {
            id: argument_id.clone(),
            label,
            ty: PropertyType::Value(PropertyValueType::Scalar(ScalarPropertyType::F32)),
            default: PropertyValue::F32(0.),
            scalars: vec![PropertyScalarSchema::default()],
            scene_bindable: true,
        };
        let schema = SceneArgumentSchema::from_property(schema)
            .expect("expression scene arguments have a supported scalar schema");
        scene.arguments.push(
            SceneArgument::computed(schema, Vec::new(), expression)
                .expect("expression scene arguments have an f32 schema"),
        );
        debug_assert!(scene_argument_expressions_valid(&scene.arguments));
        self.finish_project_edit(Some(before), None);
        Some(argument_id)
    }

    pub(crate) fn update_scene_argument_expression(
        &mut self,
        argument_id: &str,
        source: &str,
    ) -> bool {
        let Some(scene_id) = self.active_scene_id() else {
            return false;
        };
        let source = source.trim();
        if source.is_empty() {
            return false;
        }
        let Some(scene) = self.project().scenes.get(&scene_id) else {
            return false;
        };
        let Some(source) = scene.expression_from_display(source) else {
            return false;
        };
        let mut arguments = scene.arguments.clone();
        let Some(argument) = arguments.iter_mut().find(|argument| {
            argument.schema.id() == argument_id && argument.expression().is_some()
        }) else {
            return false;
        };
        if argument.expression() == Some(source.as_str()) {
            return false;
        }
        if !argument.set_expression(source) {
            return false;
        }
        if !scene_argument_expressions_valid(&arguments) {
            return false;
        }

        let key = HistoryKey::SceneArgumentExpression(scene_id, argument_id.to_owned());
        let before = self.history_snapshot_for_edit(Some(&key));
        let scene = self
            .project_mut()
            .scenes
            .get_mut(&scene_id)
            .expect("the active scene was checked");
        scene.arguments = arguments;
        self.sync_computed_scene_binding_values(scene_id);
        self.finish_project_edit(before, Some(key));
        true
    }

    pub(crate) fn rename_scene_argument(&mut self, argument_id: &str, label: &str) -> bool {
        let Some(scene_id) = self.active_scene_id() else {
            return false;
        };
        let label = label.trim();
        let Some(normalized_new) = expression::variable_name_for_label(label) else {
            return false;
        };
        let key = HistoryKey::SceneArgumentLabel(scene_id, argument_id.to_owned());
        let before = self.history_snapshot_for_edit(Some(&key));
        let Some(scene) = self.project().scenes.get(&scene_id) else {
            return false;
        };
        if scene
            .arguments
            .iter()
            .any(|argument| argument.schema.id() != argument_id && argument.schema.label() == label)
        {
            return false;
        }
        if scene.arguments.iter().any(|argument| {
            argument.schema.id() != argument_id
                && expression::variable_name_for_label(argument.schema.label())
                    == Some(normalized_new.clone())
        }) {
            return false;
        }
        let mut arguments = scene.arguments.clone();
        let Some(argument) = arguments
            .iter_mut()
            .find(|argument| argument.schema.id() == argument_id)
        else {
            return false;
        };
        if argument.schema.label() == label {
            return false;
        }
        argument.schema.rename(label.to_owned());
        if !scene_argument_expressions_valid(&arguments) {
            return false;
        }
        self.project_mut()
            .scenes
            .get_mut(&scene_id)
            .expect("the active scene was checked")
            .arguments = arguments;
        self.finish_project_edit(before, Some(key));
        true
    }

    pub(crate) fn move_scene_argument(&mut self, argument_id: &str, direction: i64) -> bool {
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

        let before = self.history_snapshot();
        let scene = self
            .project_mut()
            .scenes
            .get_mut(&scene_id)
            .expect("the active scene was checked");
        scene.arguments.swap(index, target);
        self.finish_project_edit(Some(before), None);
        true
    }

    pub(crate) fn update_scene_argument_numeric_settings(
        &mut self,
        argument_id: &str,
        settings: crate::domain::property::NumericSettings,
    ) -> bool {
        let Some(scene_id) = self.active_scene_id() else {
            return false;
        };
        let key = HistoryKey::SceneArgumentSettings(scene_id, argument_id.to_owned());
        let Some(argument) = self
            .project()
            .scenes
            .get(&scene_id)
            .and_then(|scene| scene.input_argument(argument_id))
        else {
            return false;
        };
        let Some(next_schema) = argument.schema.with_numeric_settings(settings) else {
            return false;
        };
        if next_schema == argument.schema {
            return false;
        }
        let bindings = argument.bindings.clone();
        let applied_default = next_schema.default_value().clone();
        let before = self.history_snapshot_for_edit(Some(&key));
        let argument = self
            .project_mut()
            .scenes
            .get_mut(&scene_id)
            .and_then(|scene| scene.input_argument_mut(argument_id))
            .expect("the scene argument was checked above");
        argument.schema = next_schema;
        for binding in &bindings {
            let result = self.apply_scene_binding(scene_id, binding, applied_default.clone());
            debug_assert!(
                result.is_some(),
                "validated scene binding must accept its value"
            );
        }
        self.sync_computed_scene_binding_values(scene_id);
        self.finish_project_edit(before, Some(key));
        true
    }

    pub(crate) fn update_scene_argument_default(
        &mut self,
        argument_id: &str,
        value: PropertyValue,
    ) -> bool {
        let Some(scene_id) = self.active_scene_id() else {
            return false;
        };
        let key = HistoryKey::SceneArgumentSettings(scene_id, argument_id.to_owned());
        let Some(argument) = self
            .project()
            .scenes
            .get(&scene_id)
            .and_then(|scene| scene.input_argument(argument_id))
        else {
            return false;
        };
        let bindings = argument.bindings.clone();
        let Some(next_schema) = argument.schema.with_default(&value) else {
            return false;
        };
        if next_schema == argument.schema {
            return false;
        }
        let applied_default = next_schema.default_value().clone();
        let before = self.history_snapshot_for_edit(Some(&key));
        let argument = self
            .project_mut()
            .scenes
            .get_mut(&scene_id)
            .and_then(|scene| scene.input_argument_mut(argument_id))
            .expect("the scene argument was checked above");
        argument.schema = next_schema;
        for binding in &bindings {
            let result = self.apply_scene_binding(scene_id, binding, applied_default.clone());
            debug_assert!(
                result.is_some(),
                "validated scene binding must accept its value"
            );
        }

        self.sync_computed_scene_binding_values(scene_id);
        self.finish_project_edit(before, Some(key));
        true
    }

    pub(crate) fn connect_scene_argument(
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
        if !resolved.schema.scene_bindable {
            return Err(SceneArgumentEditError::TargetNotBindable);
        }
        let Some(argument) = scene.argument(argument_id) else {
            return Err(SceneArgumentEditError::ArgumentNotFound);
        };
        let item = scene
            .document()
            .item(target.item_id())
            .ok_or(SceneArgumentEditError::TargetNotFound)?;
        if target.conflicts_with_aspect_ratio_lock(item, item.aspect_ratio_locked)
            || resolved.schema.ty() != argument.schema.ty()
        {
            return Err(SceneArgumentEditError::IncompatibleContract);
        }
        let value = argument.schema.default_value().clone();
        let before = self.history_snapshot();
        let Some(argument) = self
            .project_mut()
            .scenes
            .get_mut(&scene_id)
            .and_then(|scene| scene.argument_mut(argument_id))
        else {
            return Err(SceneArgumentEditError::ArgumentNotFound);
        };
        argument.bindings.push(target.clone());
        self.apply_scene_binding(scene_id, &target, value)
            .expect("validated scene binding must accept its value");
        self.sync_computed_scene_binding_values(scene_id);
        self.finish_project_edit(Some(before), None);
        Ok(())
    }

    pub(crate) fn disconnect_scene_argument(
        &mut self,
        argument_id: &str,
        target: &SceneBindingTarget,
    ) -> Result<(), SceneArgumentEditError> {
        let Some(scene_id) = self.active_scene_id() else {
            return Err(SceneArgumentEditError::NoActiveScene);
        };
        let before = self.history_snapshot();
        let Some(argument) = self
            .project_mut()
            .scenes
            .get_mut(&scene_id)
            .and_then(|scene| scene.argument_mut(argument_id))
        else {
            return Err(SceneArgumentEditError::ArgumentNotFound);
        };
        let previous = argument.bindings.len();
        argument.bindings.retain(|binding| binding != target);
        if argument.bindings.len() == previous {
            return Err(SceneArgumentEditError::TargetNotFound);
        }
        self.finish_project_edit(Some(before), None);
        Ok(())
    }

    pub(crate) fn remove_scene_argument(
        &mut self,
        argument_id: &str,
    ) -> Result<(), SceneArgumentEditError> {
        let Some(scene_id) = self.active_scene_id() else {
            return Err(SceneArgumentEditError::NoActiveScene);
        };
        let before = self.history_snapshot();
        let Some(scene) = self.project_mut().scenes.get_mut(&scene_id) else {
            return Err(SceneArgumentEditError::NoActiveScene);
        };
        let Some(_) = scene
            .arguments
            .iter()
            .find(|argument| argument.schema.id() == argument_id)
        else {
            return Err(SceneArgumentEditError::ArgumentNotFound);
        };
        if scene.arguments.iter().any(|argument| {
            argument.schema.id() != argument_id && argument.expression_references(argument_id)
        }) {
            return Err(SceneArgumentEditError::ReferencedByExpression);
        }
        let previous = scene.arguments.len();
        scene
            .arguments
            .retain(|argument| argument.schema.id() != argument_id);
        if scene.arguments.len() == previous {
            return Err(SceneArgumentEditError::ArgumentNotFound);
        }
        self.for_each_scene_instance_mut(scene_id, |item| {
            item.properties.remove(argument_id);
            item.animations.retain_valid(&item.properties);
        });
        remove_bindings_for_nested_argument(&mut self.project_mut().scenes, scene_id, argument_id);
        self.finish_project_edit(Some(before), None);
        Ok(())
    }
}

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
        let Some(time) = super::time::TimelineTime::from_seconds(seconds, self.frame_rate()) else {
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
    pub(crate) fn update_selected_scalar(
        &mut self,
        effect: Option<EffectInstanceId>,
        property_id: &str,
        element_id: Option<PropertyElementId>,
        scalar_index: Option<usize>,
        value: PropertyValue,
    ) -> bool {
        let targets: Vec<_> = match effect {
            Some(effect) => match self.selected_effect_instances(effect) {
                Some(targets) => targets
                    .into_iter()
                    .map(|(item, effect)| (item, Some(effect)))
                    .collect(),
                None => return false,
            },
            None => self
                .selection
                .sorted_current()
                .into_iter()
                .map(|item| (item, None))
                .collect(),
        };
        if targets.is_empty() {
            return false;
        }
        let updates: Option<Vec<_>> = targets
            .iter()
            .map(|(id, effect)| {
                let item = self.active_document().item(*id)?;
                let owner = effect.map_or(SceneBindingOwner::Item, SceneBindingOwner::Effect);
                let schema =
                    resolve_property_schema(&self.project().scenes, item, owner, property_id)?;
                let current = match effect {
                    Some(effect) => item
                        .effects
                        .iter()
                        .find(|entry| entry.id == *effect)?
                        .properties
                        .property(property_id)?,
                    None => item
                        .properties
                        .property(property_id)
                        .unwrap_or(schema.default_value()),
                };
                let next = super::scene::apply_scene_binding_value(
                    current,
                    element_id,
                    scalar_index,
                    value.clone(),
                )?;
                (schema.is_editable(scalar_index)
                    && schema.ty().allows(&next)
                    && self.value_preserves_active_bindings(*id, *effect, property_id, &next))
                .then_some((*id, *effect, next))
            })
            .collect();
        let Some(updates) = updates else {
            return false;
        };
        let key = match effect {
            Some(_) => HistoryKey::EffectsProperty(
                targets
                    .iter()
                    .map(|(id, effect)| (*id, effect.unwrap()))
                    .collect(),
                property_id.to_owned(),
            ),
            None => HistoryKey::ItemsProperty(
                targets.iter().map(|(id, _)| *id).collect(),
                property_id.to_owned(),
            ),
        };
        let before = self.history_snapshot_for_edit(Some(&key));
        let mut changed = false;
        for (id, effect, value) in updates {
            changed |= match effect {
                Some(effect) => self.active_document_mut().update_item_effect_property(
                    id,
                    effect,
                    property_id,
                    value,
                ),
                None => {
                    if let Some(schema) = self.scene_instance_property_schema(id, property_id) {
                        self.active_document_mut()
                            .item_mut(id)
                            .is_some_and(|item| set_scene_instance_override(item, &schema, value))
                    } else {
                        self.active_document_mut()
                            .update_item_property(id, property_id, value)
                    }
                }
            };
        }
        self.finish_project_edit_if_changed(changed, before, Some(key))
    }

    pub(crate) fn update_selected_property(
        &mut self,
        property_id: &str,
        value: PropertyValue,
    ) -> bool {
        let ids = self.selection.sorted_current();
        if ids.is_empty()
            || ids
                .iter()
                .any(|id| !self.value_preserves_active_bindings(*id, None, property_id, &value))
            || ids.iter().any(|id| {
                let Some(item) = self.active_document().item(*id) else {
                    return true;
                };
                let property = resolve_property_schema(
                    &self.project().scenes,
                    item,
                    SceneBindingOwner::Item,
                    property_id,
                );
                property.is_none_or(|property| {
                    !property.is_editable(None) || !property.ty.allows(&value)
                })
            })
        {
            return false;
        }
        let key = if ids.len() == 1 {
            HistoryKey::ItemProperty(ids[0], property_id.to_owned())
        } else {
            HistoryKey::ItemsProperty(ids.clone(), property_id.to_owned())
        };
        let before = self.history_snapshot_for_edit(Some(&key));
        let mut changed = false;
        for id in ids {
            let scene_schema = self.scene_instance_property_schema(id, property_id);
            changed |= if let Some(schema) = scene_schema {
                self.active_document_mut()
                    .item_mut(id)
                    .is_some_and(|item| set_scene_instance_override(item, &schema, value.clone()))
            } else {
                self.active_document_mut()
                    .update_item_property(id, property_id, value.clone())
            };
        }
        self.finish_project_edit_if_changed(changed, before, Some(key))
    }

    pub(crate) fn update_selected_aspect_ratio_locked(&mut self, locked: bool) -> bool {
        let ids = self.selection.sorted_current();
        if ids.is_empty()
            || ids.iter().any(|id| {
                self.active_document()
                    .item(*id)
                    .and_then(TimelineItem::schema)
                    .is_none_or(|schema| {
                        !schema.supports_aspect_ratio_lock()
                            || schema
                                .size_property()
                                .is_none_or(|property| !property.is_editable(None))
                    })
            })
            || (locked
                && self.active_scene_id().is_some_and(|scene_id| {
                    self.project().scenes.get(&scene_id).is_some_and(|scene| {
                        scene
                            .arguments
                            .iter()
                            .flat_map(|argument| &argument.bindings)
                            .any(|binding| {
                                ids.contains(&binding.item_id())
                                    && self.active_document().item(binding.item_id()).is_some_and(
                                        |item| binding.conflicts_with_aspect_ratio_lock(item, true),
                                    )
                            })
                    })
                }))
        {
            return false;
        }
        let key = HistoryKey::ItemsProperty(ids.clone(), "aspect_ratio_locked".to_owned());
        let before = self.history_snapshot_for_edit(Some(&key));
        let mut changed = false;
        for id in ids {
            changed |= self
                .active_document_mut()
                .update_item_aspect_ratio_locked(id, locked);
        }
        self.finish_project_edit_if_changed(changed, before, Some(key))
    }

    pub(crate) fn add_selected_effect(
        &mut self,
        plugin_id: &str,
        effect_id: &str,
    ) -> Result<EffectInstanceId, TimelineEditError> {
        let schema = self.plugins.effect(plugin_id, effect_id).ok_or_else(|| {
            TimelineEditError::PluginEffectNotFound {
                plugin_id: plugin_id.to_owned(),
                effect_id: effect_id.to_owned(),
            }
        })?;
        let id = self
            .selection
            .primary
            .ok_or(TimelineEditError::NothingSelected)?;
        let raw_effect_id = self
            .next_effect_id
            .ok_or(TimelineEditError::IdentifierExhausted)?;
        let instance_id = EffectInstanceId::new(raw_effect_id);
        let before = self.history_snapshot();
        if !self.active_document_mut().add_item_effect(
            id,
            instance_id,
            plugin_id,
            effect_id,
            schema,
        ) {
            return Err(TimelineEditError::ItemNotFound(id));
        }
        self.next_effect_id = raw_effect_id.checked_add(1).filter(|id| *id != u64::MAX);
        self.finish_project_edit(Some(before), None);
        Ok(instance_id)
    }

    pub(crate) fn update_selected_effect_property(
        &mut self,
        effect_id: EffectInstanceId,
        property_id: &str,
        value: PropertyValue,
    ) -> bool {
        let Some(effects) = self.selected_effect_instances(effect_id) else {
            return false;
        };
        if effects.iter().any(|(item_id, effect_id)| {
            !self.value_preserves_active_bindings(*item_id, Some(*effect_id), property_id, &value)
        }) {
            return false;
        }
        if effects.iter().any(|(item_id, effect_id)| {
            self.active_document()
                .item(*item_id)
                .and_then(|item| item.effects.iter().find(|effect| effect.id == *effect_id))
                .and_then(|effect| effect.schema().property(property_id))
                .is_none_or(|property| !property.is_editable(None) || !property.ty.allows(&value))
        }) {
            return false;
        }
        let key = if effects.len() == 1 {
            HistoryKey::EffectProperty(effects[0].0, effects[0].1, property_id.to_owned())
        } else {
            HistoryKey::EffectsProperty(effects.clone(), property_id.to_owned())
        };
        let before = self.history_snapshot_for_edit(Some(&key));
        let mut changed = false;
        for (item_id, effect_id) in effects {
            changed |= self.active_document_mut().update_item_effect_property(
                item_id,
                effect_id,
                property_id,
                value.clone(),
            );
        }
        self.finish_project_edit_if_changed(changed, before, Some(key))
    }

    pub(super) fn selected_effect_instances(
        &self,
        primary_effect_id: EffectInstanceId,
    ) -> Option<Vec<(ItemId, EffectInstanceId)>> {
        let primary_item_id = self.selection.primary?;
        let primary_item = self.active_document().item(primary_item_id)?;
        let effect_index = primary_item
            .effects
            .iter()
            .position(|effect| effect.id == primary_effect_id)?;
        let primary_effect = primary_item.effects.get(effect_index)?;
        let item_ids = self.selection.sorted_current();
        item_ids
            .into_iter()
            .map(|item_id| {
                let effect = self
                    .active_document()
                    .item(item_id)?
                    .effects
                    .get(effect_index)?;
                (effect.plugin_id == primary_effect.plugin_id
                    && effect.effect_id == primary_effect.effect_id)
                    .then_some((item_id, effect.id))
            })
            .collect()
    }

    pub(crate) fn set_selected_property_animation_enabled(
        &mut self,
        effect_id: Option<EffectInstanceId>,
        property_id: String,
        element_id: Option<PropertyElementId>,
        scalar_index: Option<usize>,
        enabled: bool,
    ) -> bool {
        let Some(item_id) = self.selection.primary else {
            return false;
        };
        if enabled
            && self.active_scene_has_binding(item_id, effect_id, &property_id, |binding| {
                binding.conflicts_with_animation(&property_id, element_id, scalar_index)
            })
        {
            return false;
        }
        let before = self.history_snapshot();
        let Some(schema) = self.animation_schema(item_id, effect_id, &property_id) else {
            return false;
        };
        if !schema.is_editable(scalar_index) {
            return false;
        }
        if effect_id.is_none()
            && scalar_index == Some(1)
            && self.active_document().item(item_id).is_some_and(|item| {
                item.preserves_aspect_ratio()
                    && item
                        .schema()
                        .is_some_and(|schema| schema.is_size_property(&property_id))
            })
        {
            return false;
        }
        let address = ScalarAnimationAddress::new(property_id.clone(), element_id, scalar_index);
        let changed = if !enabled {
            self.animation_store_mut(item_id, effect_id)
                .is_some_and(|animations| animations.remove(&address))
        } else {
            if !schema.is_animatable(scalar_index) {
                return false;
            }
            let Some(item) = self.active_document().item(item_id) else {
                return false;
            };
            let values = match effect_id {
                Some(effect_id) => item
                    .effects
                    .iter()
                    .find(|effect| effect.id == effect_id)
                    .map(|effect| effect.properties.clone()),
                None if item.scene_id().is_some() => {
                    materialize_scene_instance_properties(item, &self.project().scenes)
                }
                None => Some(item.properties.clone()),
            };
            let Some(values) = values else {
                return false;
            };
            let Some(value) = values
                .property(&property_id)
                .and_then(|value| value.element(element_id))
                .and_then(|value| value.scalar_at(scalar_index))
            else {
                return false;
            };
            let Some(value_type) = (match (element_id, schema.ty()) {
                (Some(_), PropertyType::Array { element_type, .. })
                | (None, PropertyType::Value(element_type)) => element_type.scalar_at(scalar_index),
                _ => None,
            }) else {
                return false;
            };
            let Some(track) = ScalarTrack::from_value(value.clone(), value_type) else {
                return false;
            };
            self.animation_store_mut(item_id, effect_id)
                .is_some_and(|animations| animations.insert(address, track))
        };
        self.finish_project_edit_if_changed(changed, Some(before), None)
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn set_selected_property_animation_stop(
        &mut self,
        effect_id: Option<EffectInstanceId>,
        property_id: String,
        element_id: Option<PropertyElementId>,
        scalar_index: Option<usize>,
        index: usize,
        value: PropertyValue,
        focused_segment: Option<usize>,
    ) -> bool {
        let Some(item_id) = self.selection.primary else {
            return false;
        };
        let Some(item) = self.active_document().item(item_id) else {
            return false;
        };
        let address = ScalarAnimationAddress::new(property_id.clone(), element_id, scalar_index);
        let Some(position) = self
            .animation_track(item_id, effect_id, &address)
            .and_then(|track| track.stops().get(index))
            .map(|stop| stop.position())
        else {
            return false;
        };
        let stop_frame = Frame::new(item.animation_timeline_frame(position).round().max(0.) as u64);
        let key = HistoryKey::AnimationStopValue(
            item_id,
            effect_id,
            property_id.clone(),
            element_id,
            scalar_index,
            stop_frame,
        );
        let before = self.history_snapshot_for_edit(Some(&key));
        let Some(schema) = self.animation_schema(item_id, effect_id, &property_id) else {
            return false;
        };
        let changed = schema.is_editable(scalar_index)
            && schema.scalar_constraints(scalar_index).allows(&value)
            && self
                .animation_track_mut(item_id, effect_id, &address)
                .is_some_and(|animation| animation.set_stop(index, value, focused_segment));
        self.finish_project_edit_if_changed(changed, before, Some(key))
    }

    pub(crate) fn set_selected_property_animation_pair_stop_at(
        &mut self,
        property_id: &str,
        element_id: Option<PropertyElementId>,
        position: f32,
        value: [f32; 2],
    ) -> bool {
        let Some(item_id) = self.selection.primary else {
            return false;
        };
        let Some(item) = self.active_document().item(item_id) else {
            return false;
        };
        let stop_frame = Frame::new(item.animation_timeline_frame(position).round().max(0.) as u64);
        let key = HistoryKey::AnimationPairStopValue(
            item_id,
            property_id.to_owned(),
            element_id,
            stop_frame,
        );
        let before = self.history_snapshot_for_edit(Some(&key));
        let Some(schema) = self.animation_schema(item_id, None, property_id) else {
            return false;
        };
        let editable = [0_usize, 1].map(|scalar_index| {
            schema.is_editable(Some(scalar_index))
                && schema
                    .scalar_constraints(Some(scalar_index))
                    .allows(&PropertyValue::F32(value[scalar_index]))
        });
        let mut changed = false;
        for scalar_index in 0..2 {
            if !editable[scalar_index] {
                continue;
            }
            let address = ScalarAnimationAddress::new(property_id, element_id, Some(scalar_index));
            let Some(index) = self
                .animation_track(item_id, None, &address)
                .and_then(|track| track.stop_index_at(position))
            else {
                continue;
            };
            changed |= self
                .animation_track_mut(item_id, None, &address)
                .is_some_and(|track| {
                    track.set_stop_exact(index, PropertyValue::F32(value[scalar_index]))
                });
        }
        self.finish_project_edit_if_changed(changed, before, Some(key))
    }

    pub(crate) fn insert_selected_property_animation_stop(
        &mut self,
        effect_id: Option<EffectInstanceId>,
        property_id: String,
        element_id: Option<PropertyElementId>,
        scalar_index: Option<usize>,
        position: f32,
        value: PropertyValue,
    ) -> Option<usize> {
        let item_id = self.selection.primary?;
        let stop_frame = self
            .active_document()
            .item(item_id)
            .map(|item| Frame::new(item.animation_timeline_frame(position).round().max(0.) as u64))
            .unwrap_or(self.playhead);
        let key = HistoryKey::AnimationStopValue(
            item_id,
            effect_id,
            property_id.clone(),
            element_id,
            scalar_index,
            stop_frame,
        );
        let before = self.history_snapshot_for_edit(Some(&key));
        let inserted = self
            .animation_schema(item_id, effect_id, &property_id)
            .filter(|schema| {
                schema.is_editable(scalar_index)
                    && schema.scalar_constraints(scalar_index).allows(&value)
            })
            .and_then(|_| {
                self.animation_track_mut(
                    item_id,
                    effect_id,
                    &ScalarAnimationAddress::new(property_id, element_id, scalar_index),
                )
            })
            .and_then(|animation| animation.insert_stop(position, value));
        if inserted.is_some() {
            self.finish_project_edit(before, Some(key));
        }
        inserted
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn set_selected_animation_handle(
        &mut self,
        effect_id: Option<EffectInstanceId>,
        property_id: String,
        element_id: Option<PropertyElementId>,
        scalar_index: Option<usize>,
        segment: usize,
        handle: BezierHandle,
        position: [f32; 2],
    ) -> bool {
        let Some(item_id) = self.selection.primary else {
            return false;
        };
        let key = HistoryKey::AnimationHandle(
            item_id,
            effect_id,
            property_id.clone(),
            element_id,
            scalar_index,
            segment,
            handle,
        );
        let before = self.history_snapshot_for_edit(Some(&key));
        self.edit_selected_animation(
            item_id,
            effect_id,
            ScalarAnimationAddress::new(property_id.as_str(), element_id, scalar_index),
            before,
            Some(key),
            |animation| animation.set_segment_handle(segment, handle, position),
        )
    }

    pub(crate) fn set_selected_animation_interpolation(
        &mut self,
        effect_id: Option<EffectInstanceId>,
        property_id: String,
        element_id: Option<PropertyElementId>,
        scalar_index: Option<usize>,
        segment: usize,
        interpolation: SegmentInterpolation,
    ) -> bool {
        let Some(item_id) = self.selection.primary else {
            return false;
        };
        let before = self.history_snapshot();
        self.edit_selected_animation(
            item_id,
            effect_id,
            ScalarAnimationAddress::new(property_id.as_str(), element_id, scalar_index),
            Some(before),
            None,
            |animation| animation.set_segment_interpolation(segment, interpolation),
        )
    }

    pub(crate) fn remove_selected_animation_stop(
        &mut self,
        effect_id: Option<EffectInstanceId>,
        property_id: String,
        element_id: Option<PropertyElementId>,
        scalar_index: Option<usize>,
        stop: usize,
    ) -> bool {
        let Some(item_id) = self.selection.primary else {
            return false;
        };
        let before = self.history_snapshot();
        self.edit_selected_animation(
            item_id,
            effect_id,
            ScalarAnimationAddress::new(property_id.as_str(), element_id, scalar_index),
            Some(before),
            None,
            |animation| animation.remove_stop(stop),
        )
    }

    pub(crate) fn move_selected_animation_stop(
        &mut self,
        effect_id: Option<EffectInstanceId>,
        property_id: String,
        element_id: Option<PropertyElementId>,
        scalar_index: Option<usize>,
        stop: usize,
        position: f32,
    ) -> bool {
        let Some(item_id) = self.selection.primary else {
            return false;
        };
        let key = HistoryKey::AnimationStopPosition(
            item_id,
            effect_id,
            property_id.clone(),
            element_id,
            scalar_index,
            stop,
        );
        let before = self.history_snapshot_for_edit(Some(&key));
        self.edit_selected_animation(
            item_id,
            effect_id,
            ScalarAnimationAddress::new(property_id.as_str(), element_id, scalar_index),
            before,
            Some(key),
            |animation| animation.move_stop(stop, position),
        )
    }

    pub(crate) fn remove_selected_effect(&mut self, effect_id: EffectInstanceId) -> bool {
        let Some(item_id) = self.selection.primary else {
            return false;
        };
        let before = self.history_snapshot();
        let changed = self
            .active_document_mut()
            .remove_item_effect(item_id, effect_id);
        if changed
            && let Some(scene_id) = self.active_scene_id()
            && let Some(scene) = self.project_mut().scenes.get_mut(&scene_id)
        {
            for argument in &mut scene.arguments {
                argument.bindings.retain(|binding| {
                    !(binding.item_id() == item_id
                        && binding.owner() == SceneBindingOwner::Effect(effect_id))
                });
            }
        }
        self.finish_project_edit_if_changed(changed, Some(before), None)
    }

    pub(crate) fn resize_item(
        &mut self,
        origin: &TimelineItem,
        edge: ResizeEdge,
        pointer: Frame,
    ) -> bool {
        let id = origin.id;
        let scene_limit = self
            .active_document()
            .item(id)
            .and_then(TimelineItem::scene_id)
            .and_then(|scene_id| self.project().scenes.get(&scene_id))
            .map(SceneDefinition::duration);
        if scene_limit.is_some() && edge == ResizeEdge::Left {
            return false;
        }
        let pointer = if let Some(limit) = scene_limit {
            Frame::new(
                pointer
                    .get()
                    .min(origin.start.get().saturating_add(limit.get())),
            )
        } else {
            pointer
        };
        let key = HistoryKey::ItemResize(id, edge);
        let before = self.history_snapshot_for_edit(Some(&key));
        let changed = self
            .active_document_mut()
            .resize_item_from(origin, edge, pointer);
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
