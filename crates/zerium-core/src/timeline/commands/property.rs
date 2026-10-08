use crate::property::PropertyValue;
use crate::timeline::history::HistoryKey;
use crate::timeline::{
    AspectRatio, EditScope, EffectInstanceId, ItemId, PropertyAddress, TimelineEditor,
};

use super::TimelineEditError;

enum PropertyEdit {
    Set(PropertyValue),
    Reset,
}

impl TimelineEditor {
    pub fn update_property(
        &mut self,
        scope: EditScope,
        address: &PropertyAddress,
        value: PropertyValue,
    ) -> bool {
        self.edit_property(scope, address, value).unwrap_or(false)
    }

    /// Prepare every final value before mutating the document. The address defines
    /// which component is editable and, for linked pairs, which axis drives it.
    pub fn edit_property(
        &mut self,
        scope: EditScope,
        address: &PropertyAddress,
        value: PropertyValue,
    ) -> Result<bool, TimelineEditError> {
        self.apply_property_edit(scope, address, PropertyEdit::Set(value))
    }

    pub fn reset_property(
        &mut self,
        scope: EditScope,
        address: &PropertyAddress,
    ) -> Result<bool, TimelineEditError> {
        self.apply_property_edit(scope, address, PropertyEdit::Reset)
    }

    fn apply_property_edit(
        &mut self,
        scope: EditScope,
        address: &PropertyAddress,
        edit: PropertyEdit,
    ) -> Result<bool, TimelineEditError> {
        if !self.is_item_selected(address.item_id) {
            return Ok(false);
        }
        let targets = self
            .source_items_in_scope(scope)
            .map(|item| self.corresponding_property_address(address, item.id))
            .collect::<Option<Vec<_>>>();
        let Some(targets) = targets.filter(|targets| !targets.is_empty()) else {
            return Ok(false);
        };
        let updates: Option<Vec<_>> = targets
            .iter()
            .map(|address| {
                let id = address.item_id;
                let effect = address.effect_id;
                let path = address.path();
                let item = self.item(id)?;
                let schema = self.property_schema(id, effect, path.property_id())?;
                let current = item
                    .property_values(effect)?
                    .property(path.property_id())
                    .unwrap_or_else(|| schema.default_value());
                let value = match &edit {
                    PropertyEdit::Set(value) => value.clone(),
                    PropertyEdit::Reset => schema
                        .default_value()
                        .element(path.element_id())?
                        .scalar_at(path.scalar_index())?
                        .clone(),
                };
                let mut next =
                    current.replaced_at(path.element_id(), path.scalar_index(), value)?;
                if !schema.is_editable(path.scalar_index()) {
                    return None;
                }
                if let Some(ratio) = item.aspect_ratio(effect).filter(|_| {
                    item.aspect_lock_property(effect)
                        .is_some_and(|property| property.id() == path.property_id())
                }) {
                    if !schema.is_editable(None) {
                        return None;
                    }
                    next = ratio.constrain(schema, &next, path.scalar_index())?;
                }
                if !schema.accepts_value(&next)
                    || !self.value_preserves_active_bindings(id, effect, path.property_id(), &next)
                {
                    return None;
                }
                // Only an explicit reset removes a scene instance's override. An
                // assigned value remains assigned even when equal to the default.
                let inherit = matches!(edit, PropertyEdit::Reset)
                    && effect.is_none()
                    && item.scene_id().is_some()
                    && path.element_id().is_none()
                    && path.scalar_index().is_none();
                Some((id, effect, schema.clone(), (!inherit).then_some(next)))
            })
            .collect();
        let Some(updates) = updates else {
            return Ok(false);
        };
        let key = HistoryKey::Property(
            targets
                .iter()
                .map(|target| (target.item_id, target.effect_id))
                .collect(),
            address.property_id.clone(),
        );
        self.try_edit_project(Some(key), |editor| {
            let changed = editor.active_document_mut().set_properties(&updates)?;
            Ok((changed, changed))
        })
    }

    /// An explicit effect identifies the position and schema to match across
    /// selected items. Item properties apply directly to every selected item.
    fn property_owners(
        &self,
        scope: EditScope,
        effect_id: Option<EffectInstanceId>,
    ) -> Option<Vec<(ItemId, Option<EffectInstanceId>)>> {
        let targets = match effect_id {
            Some(id) => self
                .effect_instances(scope, id)?
                .1
                .into_iter()
                .map(|(item, effect)| (item, Some(effect)))
                .collect(),
            None => scope
                .item_ids(self)
                .into_iter()
                .map(|item| (item, None))
                .collect::<Vec<_>>(),
        };
        (!targets.is_empty()).then_some(targets)
    }

    pub fn update_aspect_ratio_locked(
        &mut self,
        scope: EditScope,
        effect_id: Option<EffectInstanceId>,
        locked: bool,
    ) -> bool {
        let Some(targets) = self.property_owners(scope, effect_id) else {
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
        self.edit_project_if_changed(Some(key), |editor| {
            let mut changed = false;
            for (id, effect, ratio) in updates {
                changed |= editor
                    .active_document_mut()
                    .set_item_aspect_ratio(id, effect, ratio);
            }
            changed
        })
    }
}
