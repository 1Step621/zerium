use super::*;
use crate::{property::PropertyPath, timeline::AspectRatio};

impl TimelineEditor {
    pub fn update_selected_property(
        &mut self,
        effect_id: Option<EffectInstanceId>,
        path: PropertyPath,
        value: PropertyValue,
    ) -> bool {
        self.edit_selected_property(effect_id, path, value)
            .unwrap_or(false)
    }

    /// Prepare every final value before mutating the selection. The path defines
    /// which component is editable and, for linked pairs, which axis drives it.
    pub fn edit_selected_property(
        &mut self,
        effect_id: Option<EffectInstanceId>,
        path: PropertyPath,
        value: PropertyValue,
    ) -> Result<bool, TimelineEditError> {
        self.edit_selected_property_with(effect_id, path, |_, _| Some(value.clone()))
    }

    pub fn reset_selected_property(
        &mut self,
        effect_id: Option<EffectInstanceId>,
        path: PropertyPath,
    ) -> Result<bool, TimelineEditError> {
        let default_path = path.clone();
        self.edit_selected_property_with(effect_id, path, |schema, _| {
            schema
                .default_value()
                .element(default_path.element_id())?
                .scalar_at(default_path.scalar_index())
                .cloned()
        })
    }

    fn edit_selected_property_with(
        &mut self,
        effect_id: Option<EffectInstanceId>,
        path: PropertyPath,
        value: impl Fn(&PropertySchema, &PropertyValue) -> Option<PropertyValue>,
    ) -> Result<bool, TimelineEditError> {
        let Some(targets) = self.selected_property_owners(effect_id) else {
            return Ok(false);
        };
        let updates: Option<Vec<_>> = targets
            .iter()
            .map(|(id, effect)| {
                let item = self.active_document().item(*id)?;
                let owner = effect.map_or(SceneBindingOwner::Item, SceneBindingOwner::Effect);
                let schema = resolve_property_schema(
                    &self.project().scenes,
                    item,
                    owner,
                    path.property_id(),
                )?;
                let current = item
                    .property_values(*effect)?
                    .property(path.property_id())
                    .unwrap_or(schema.default_value());
                let mut next = current.replaced_at(
                    path.element_id(),
                    path.scalar_index(),
                    value(schema, current)?,
                )?;
                if !schema.is_editable(path.scalar_index()) {
                    return None;
                }
                if let Some(ratio) = item.aspect_ratio(*effect).filter(|_| {
                    item.aspect_lock_property(*effect)
                        .is_some_and(|property| property.id() == path.property_id())
                }) {
                    if !schema.is_editable(None) {
                        return None;
                    }
                    next = ratio.constrain(schema, &next, path.scalar_index())?;
                }
                (schema.accepts_value(&next)
                    && self.value_preserves_active_bindings(
                        *id,
                        *effect,
                        path.property_id(),
                        &next,
                    ))
                .then_some((*id, *effect, schema.clone(), next))
            })
            .collect();
        let Some(updates) = updates else {
            return Ok(false);
        };
        let key = HistoryKey::Property(targets, path.property_id().to_owned());
        let before = self.history_snapshot_for_edit(Some(&key));
        let changed = self.active_document_mut().set_properties(&updates)?;
        Ok(self.finish_project_edit_if_changed(changed, before, Some(key)))
    }

    /// Resolve an effect by its position and schema on the primary item, so each
    /// selected item edits its own corresponding instance.
    pub fn selected_property_owners(
        &self,
        effect_id: Option<EffectInstanceId>,
    ) -> Option<Vec<(ItemId, Option<EffectInstanceId>)>> {
        let targets = match effect_id {
            Some(id) => self
                .selected_effect_instances(id)?
                .into_iter()
                .map(|(item, effect)| (item, Some(effect)))
                .collect(),
            None => self
                .selection
                .sorted_current()
                .into_iter()
                .map(|item| (item, None))
                .collect::<Vec<_>>(),
        };
        (!targets.is_empty()).then_some(targets)
    }

    pub fn update_selected_aspect_ratio_locked(
        &mut self,
        effect_id: Option<EffectInstanceId>,
        locked: bool,
    ) -> bool {
        let Some(targets) = self.selected_property_owners(effect_id) else {
            return false;
        };
        let updates: Option<Vec<_>> = targets
            .iter()
            .map(|(id, effect)| {
                let item = self.active_document().item(*id)?;
                let property = item
                    .aspect_lock_property(*effect)
                    .filter(|property| property.is_editable(None))?;
                let ratio = if locked {
                    Some(match item.aspect_ratio(*effect) {
                        Some(ratio) => ratio,
                        None => AspectRatio::from_value(
                            item.property_values(*effect)?.property(property.id())?,
                        )?,
                    })
                } else {
                    None
                };
                Some((*id, *effect, ratio))
            })
            .collect();
        let Some(updates) = updates else {
            return false;
        };
        let key = HistoryKey::AspectRatioLock(targets);
        let before = self.history_snapshot_for_edit(Some(&key));
        let mut changed = false;
        for (id, effect, ratio) in updates {
            changed |= self
                .active_document_mut()
                .set_item_aspect_ratio(id, effect, ratio);
        }
        self.finish_project_edit_if_changed(changed, before, Some(key))
    }
}
