use crate::property::PropertyOverrides;
use crate::timeline::{
    EffectInstance, EffectInstanceId, ItemId, SceneBindingOwner, TimelineEditError, TimelineEditor,
    editing::history::EditImpact,
};

impl TimelineEditor {
    pub fn toggle_effect_visibility(
        &mut self,
        item_id: ItemId,
        effect_id: EffectInstanceId,
    ) -> bool {
        if !self.is_item_selected(item_id) {
            return false;
        }
        let Ok(mut edit) = self.active_document().prepare_item_edit(item_id) else {
            return false;
        };
        let Some(effect) = edit.item.effect_mut(effect_id) else {
            return false;
        };
        effect.hidden = !effect.hidden;
        self.commit_item_edits(None, [edit]).unwrap_or(false)
    }

    /// Append copies with fresh project-wide IDs, as a single undoable edit.
    pub fn paste_effects(
        &mut self,
        id: ItemId,
        sources: &[EffectInstance],
    ) -> Result<bool, TimelineEditError> {
        if !self.is_item_selected(id) {
            return Err(TimelineEditError::NothingSelected);
        }
        let item = self.item(id).ok_or(TimelineEditError::ItemNotFound(id))?;
        if item.scene_id().is_none() && item.schema().is_none_or(|schema| schema.render().is_none())
        {
            return Err(TimelineEditError::NonVisualItem);
        }
        if sources.is_empty() {
            return Ok(false);
        }
        for source in sources {
            if self
                .plugins
                .effect(&source.plugin_id, source.schema().id())
                .is_none()
            {
                return Err(TimelineEditError::PluginEffectNotFound {
                    plugin_id: source.plugin_id.clone(),
                    effect_id: source.schema().id().to_owned(),
                });
            }
        }
        let duration = item.duration(self.frame_rate());
        let mut next_id = self.next_effect_id;
        let effects = sources
            .iter()
            .map(|source| {
                let raw_id = next_id.ok_or(TimelineEditError::IdentifierExhausted)?;
                next_id = raw_id.checked_add(1).filter(|id| *id != u64::MAX);
                let mut effect = source.clone();
                effect.id = EffectInstanceId::new(raw_id);
                effect.animations.fit_duration(duration);
                Ok(effect)
            })
            .collect::<Result<Vec<_>, TimelineEditError>>()?;
        Ok(
            self.edit_project_if_changed(EditImpact::MediaDependencies, None, |editor| {
                editor
                    .active_document_mut()
                    .item_mut(id)
                    .expect("paste target was validated before mutation")
                    .effects
                    .extend(effects);
                editor.next_effect_id = next_id;
                true
            }),
        )
    }

    pub fn move_effect(
        &mut self,
        item_id: ItemId,
        effect_id: EffectInstanceId,
        offset: i32,
    ) -> bool {
        let Some(target_index) = self.effect_move_target(item_id, effect_id, offset) else {
            return false;
        };
        self.edit_project_if_changed(EditImpact::Values, None, |editor| {
            editor
                .active_document_mut()
                .item_mut(item_id)
                .is_some_and(|item| item.move_effect(effect_id, target_index))
        })
    }

    pub fn add_item_effect(
        &mut self,
        id: ItemId,
        plugin_id: &str,
        effect_id: &str,
        properties: PropertyOverrides,
    ) -> Result<EffectInstanceId, TimelineEditError> {
        let schema = self.plugins.effect(plugin_id, effect_id).ok_or_else(|| {
            TimelineEditError::PluginEffectNotFound {
                plugin_id: plugin_id.to_owned(),
                effect_id: effect_id.to_owned(),
            }
        })?;
        crate::timeline::model::item::property::validate_initial_properties(
            &properties,
            schema.properties(),
        )?;
        if !self.is_item_selected(id) {
            return Err(TimelineEditError::NothingSelected);
        }
        let raw_effect_id = self
            .next_effect_id
            .ok_or(TimelineEditError::IdentifierExhausted)?;
        let instance_id = EffectInstanceId::new(raw_effect_id);
        self.try_edit_project(EditImpact::MediaDependencies, None, |editor| {
            let frame_rate = editor.frame_rate();
            if !editor
                .active_document_mut()
                .item_mut(id)
                .is_some_and(|item| {
                    item.add_effect(instance_id, plugin_id, schema, properties, frame_rate)
                })
            {
                return Err(TimelineEditError::ItemNotFound(id));
            }
            editor.next_effect_id = raw_effect_id.checked_add(1).filter(|id| *id != u64::MAX);
            Ok((instance_id, true))
        })
    }

    pub(in crate::timeline) fn effect_move_target(
        &self,
        item_id: ItemId,
        effect_id: EffectInstanceId,
        offset: i32,
    ) -> Option<usize> {
        if !self.is_item_selected(item_id) {
            return None;
        }
        let effects = &self.item(item_id)?.effects;
        let index = effects.iter().position(|effect| effect.id == effect_id)?;
        let target = index.checked_add_signed(offset as isize)?;
        (target < effects.len() && target != index).then_some(target)
    }

    pub fn remove_item_effect(&mut self, item_id: ItemId, effect_id: EffectInstanceId) -> bool {
        if !self.is_item_selected(item_id) {
            return false;
        }
        self.edit_project_if_changed(EditImpact::MediaDependencies, None, |editor| {
            let changed = editor
                .active_document_mut()
                .item_mut(item_id)
                .is_some_and(|item| item.remove_effect(effect_id));
            if changed && editor.active_edit_target == Some((item_id, effect_id)) {
                editor.active_edit_target = None;
            }
            if changed {
                for argument in editor.active_scene_mut().variables_mut() {
                    argument.bindings.retain(|binding| {
                        !(binding.item_id() == item_id
                            && binding.owner() == SceneBindingOwner::Effect(effect_id))
                    });
                }
            }
            changed
        })
    }
}
