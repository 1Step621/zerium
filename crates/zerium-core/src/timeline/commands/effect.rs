use crate::timeline::{
    EditScope, EffectInstance, EffectInstanceId, ItemId, SceneBindingOwner, TimelineEditor,
};

use super::TimelineEditError;

impl TimelineEditor {
    /// Append copies with fresh project-wide IDs, as a single undoable edit.
    pub fn paste_effects(
        &mut self,
        id: ItemId,
        sources: &[EffectInstance],
        hidden: &[EffectInstanceId],
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
                .effect(&source.plugin_id, &source.effect_id)
                .is_none()
            {
                return Err(TimelineEditError::PluginEffectNotFound {
                    plugin_id: source.plugin_id.clone(),
                    effect_id: source.effect_id.clone(),
                });
            }
        }
        let mut next_id = self.next_effect_id;
        let mut hidden_copies = Vec::new();
        let effects = sources
            .iter()
            .map(|source| {
                let raw_id = next_id.ok_or(TimelineEditError::IdentifierExhausted)?;
                next_id = raw_id.checked_add(1).filter(|id| *id != u64::MAX);
                let mut effect = source.clone();
                effect.id = EffectInstanceId::new(raw_id);
                if hidden.contains(&source.id) {
                    hidden_copies.push(effect.id);
                }
                Ok(effect)
            })
            .collect::<Result<Vec<_>, TimelineEditError>>()?;
        Ok(self.edit_project_if_changed(None, |editor| {
            editor
                .active_document_mut()
                .item_mut(id)
                .expect("paste target was validated before mutation")
                .effects
                .extend(effects);
            editor.next_effect_id = next_id;
            editor.visibility.toggle_effects(hidden_copies);
            true
        }))
    }

    pub fn move_effect(
        &mut self,
        scope: EditScope,
        effect_id: EffectInstanceId,
        offset: i32,
    ) -> bool {
        let Some((target_index, effects)) = self.effect_move_target(scope, effect_id, offset)
        else {
            return false;
        };
        self.edit_project_if_changed(None, |editor| {
            let mut changed = false;
            for (item_id, effect_id) in effects {
                changed |=
                    editor
                        .active_document_mut()
                        .move_item_effect(item_id, effect_id, target_index);
            }
            changed
        })
    }

    pub fn add_item_effect(
        &mut self,
        id: ItemId,
        plugin_id: &str,
        effect_id: &str,
    ) -> Result<EffectInstanceId, TimelineEditError> {
        let schema = self.plugins.effect(plugin_id, effect_id).ok_or_else(|| {
            TimelineEditError::PluginEffectNotFound {
                plugin_id: plugin_id.to_owned(),
                effect_id: effect_id.to_owned(),
            }
        })?;
        if !self.is_item_selected(id) {
            return Err(TimelineEditError::NothingSelected);
        }
        let raw_effect_id = self
            .next_effect_id
            .ok_or(TimelineEditError::IdentifierExhausted)?;
        let instance_id = EffectInstanceId::new(raw_effect_id);
        self.try_edit_project(None, |editor| {
            if !editor.active_document_mut().add_item_effect(
                id,
                instance_id,
                plugin_id,
                effect_id,
                schema,
            ) {
                return Err(TimelineEditError::ItemNotFound(id));
            }
            editor.next_effect_id = raw_effect_id.checked_add(1).filter(|id| *id != u64::MAX);
            Ok((instance_id, true))
        })
    }

    pub(in crate::timeline) fn effect_move_target(
        &self,
        scope: EditScope,
        effect_id: EffectInstanceId,
        offset: i32,
    ) -> Option<(usize, Vec<(ItemId, EffectInstanceId)>)> {
        let (source_index, effects) = self.effect_instances(scope, effect_id)?;
        let target_index = source_index.checked_add_signed(offset as isize)?;
        effects
            .iter()
            .all(|(id, _)| {
                self.item(*id)
                    .is_some_and(|item| target_index < item.effects.len())
            })
            .then_some((target_index, effects))
    }

    pub(in crate::timeline) fn effect_instances(
        &self,
        scope: EditScope,
        effect_id: EffectInstanceId,
    ) -> Option<(usize, Vec<(ItemId, EffectInstanceId)>)> {
        let item_ids = scope.item_ids(self);
        let (index, source) = item_ids.iter().find_map(|item_id| {
            self.item(*item_id)?
                .effects
                .iter()
                .enumerate()
                .find(|(_, effect)| effect.id == effect_id)
        })?;
        let instances = item_ids
            .into_iter()
            .map(|item_id| {
                let effect = self.active_document().item(item_id)?.effects.get(index)?;
                (effect.plugin_id == source.plugin_id && effect.effect_id == source.effect_id)
                    .then_some((item_id, effect.id))
            })
            .collect::<Option<Vec<_>>>()?;
        Some((index, instances))
    }

    pub fn remove_item_effect(&mut self, item_id: ItemId, effect_id: EffectInstanceId) -> bool {
        if !self.is_item_selected(item_id) {
            return false;
        }
        self.edit_project_if_changed(None, |editor| {
            let changed = editor
                .active_document_mut()
                .remove_item_effect(item_id, effect_id);
            if changed && editor.active_edit_target == Some((item_id, effect_id)) {
                editor.active_edit_target = None;
            }
            if changed
                && let Some(scene_id) = editor.active_scene_id()
                && let Some(scene) = editor.project_mut().scenes.get_mut(&scene_id)
            {
                for argument in &mut scene.arguments {
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
