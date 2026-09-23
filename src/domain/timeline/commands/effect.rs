use super::*;

impl TimelineEditor {
    pub(crate) fn set_effect_asset(
        &mut self,
        item_id: ItemId,
        effect_id: EffectInstanceId,
        imported: ImportedMedia,
    ) -> Result<(), TimelineEditError> {
        let before = self.history_snapshot();
        if !self
            .active_document_mut()
            .set_effect_asset(item_id, effect_id, imported)
        {
            return Err(TimelineEditError::IncompatibleMedia);
        }
        self.finish_project_edit(Some(before), None);
        Ok(())
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

    pub(in crate::domain::timeline) fn selected_effect_instances(
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
}
