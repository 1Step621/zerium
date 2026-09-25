use super::*;

impl TimelineEditor {
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
                let next = crate::domain::timeline::scene::apply_scene_binding_value(
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
}
