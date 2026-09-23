use super::*;

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

    pub(super) fn scene_instance_property_schema(
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

    pub(super) fn animation_schema(
        &self,
        item_id: ItemId,
        effect_id: Option<EffectInstanceId>,
        property_id: &str,
    ) -> Option<PropertySchema> {
        let item = self.active_document().item(item_id)?;
        let owner = effect_id.map_or(SceneBindingOwner::Item, SceneBindingOwner::Effect);
        resolve_property_schema(&self.project().scenes, item, owner, property_id).cloned()
    }

    pub(super) fn animation_store_mut(
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

    pub(super) fn animation_track(
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

    pub(super) fn animation_track_mut(
        &mut self,
        item_id: ItemId,
        effect_id: Option<EffectInstanceId>,
        address: &ScalarAnimationAddress,
    ) -> Option<&mut ScalarTrack> {
        self.animation_store_mut(item_id, effect_id)?
            .track_mut(address)
    }

    pub(super) fn edit_selected_animation(
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
            configurations: vec![PropertyConfiguration {
                scene_bindable: true,
                editable: true,
                animatable: preset.scalar().is_interpolatable(),
                ..Default::default()
            }],
        };
        let schema = schema
            .for_scene_argument()
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
            configurations: vec![PropertyConfiguration {
                scene_bindable: true,
                ..Default::default()
            }],
        };
        let schema = schema
            .for_scene_argument()
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
        argument.schema.label = label.to_owned();
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
        let Some(next_schema) = argument.schema.with_scene_numeric_settings(settings) else {
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
        let Some(next_schema) = argument.schema.with_scene_default(&value) else {
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
        if !resolved.schema.is_scene_bindable(None) {
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
