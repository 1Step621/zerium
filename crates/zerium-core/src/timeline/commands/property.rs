use crate::property::PropertyValue;
use crate::timeline::history::HistoryKey;
use crate::timeline::{AspectRatio, EffectInstanceId, ItemId, PropertyAddress, TimelineEditor};

use super::TimelineEditError;

enum PropertyEdit {
    Set(PropertyValue),
    Reset,
}

impl TimelineEditor {
    pub fn update_property(&mut self, address: &PropertyAddress, value: PropertyValue) -> bool {
        self.edit_property(address, value).unwrap_or(false)
    }

    pub fn edit_property(
        &mut self,
        address: &PropertyAddress,
        value: PropertyValue,
    ) -> Result<bool, TimelineEditError> {
        self.apply_property_edit(address, PropertyEdit::Set(value))
    }

    pub fn reset_property(&mut self, address: &PropertyAddress) -> Result<bool, TimelineEditError> {
        self.apply_property_edit(address, PropertyEdit::Reset)
    }

    fn apply_property_edit(
        &mut self,
        address: &PropertyAddress,
        edit: PropertyEdit,
    ) -> Result<bool, TimelineEditError> {
        if !self.is_item_selected(address.item_id) {
            return Ok(false);
        }
        let mut candidate = self.active_document().prepare_item_edit(address.item_id)?;
        if !self.prepare_property_edit(address, edit, &mut candidate) {
            return Ok(false);
        }
        let key = HistoryKey::Property(
            address.item_id,
            address.effect_id,
            address.property_id.clone(),
        );
        self.try_edit_project(Some(key), |editor| {
            let changed = editor
                .active_document_mut()
                .commit_item_edits([candidate])?;
            Ok((changed, changed))
        })
    }

    pub(super) fn prepare_property_value(
        &self,
        address: &PropertyAddress,
        value: PropertyValue,
        candidate: &mut super::super::document::ItemEdit,
    ) -> bool {
        self.prepare_property_edit(address, PropertyEdit::Set(value), candidate)
    }

    fn prepare_property_edit(
        &self,
        address: &PropertyAddress,
        edit: PropertyEdit,
        candidate: &mut super::super::document::ItemEdit,
    ) -> bool {
        let id = address.item_id;
        let effect = address.effect_id;
        if !self.is_item_selected(id) {
            return false;
        }
        let item = &candidate.item;
        let Some(schema) = self.property_schema(address) else {
            return false;
        };
        let Some(properties) = item.property_values(effect) else {
            return false;
        };
        let current = properties
            .property(&address.property_id)
            .cloned()
            .unwrap_or_else(|| schema.default_value());
        // Only an explicit reset removes a scene instance's override.
        let inherit = matches!(edit, PropertyEdit::Reset)
            && effect.is_none()
            && item.scene_id().is_some()
            && address.element_id.is_none()
            && address.scalar_index.is_none();
        let value = match edit {
            PropertyEdit::Set(value) => Some(value),
            PropertyEdit::Reset => schema
                .default_value()
                .element(address.element_id)
                .and_then(|value| value.scalar_at(address.scalar_index))
                .cloned(),
        };
        let Some(mut next) = value
            .and_then(|value| current.replaced_at(address.element_id, address.scalar_index, value))
        else {
            return false;
        };
        if !schema.is_editable(address.scalar_index) {
            return false;
        }
        if let Some(ratio) = item.aspect_ratio(effect).filter(|_| {
            item.aspect_lock_property(effect)
                .is_some_and(|property| property.id() == address.property_id)
        }) {
            if !schema.is_editable(None) {
                return false;
            }
            let Some(constrained) = ratio.constrain(schema, &next, address.scalar_index) else {
                return false;
            };
            next = constrained;
        }
        if !schema.accepts_value(&next)
            || !self.value_preserves_active_bindings(id, effect, &address.property_id, &next)
        {
            return false;
        }
        candidate.set_property(effect, schema, (!inherit).then_some(next));
        true
    }

    pub fn update_aspect_ratio_locked(
        &mut self,
        id: ItemId,
        effect: Option<EffectInstanceId>,
        locked: bool,
    ) -> bool {
        if !self.is_item_selected(id) {
            return false;
        }
        let item = self.item(id).expect("selected item must exist");
        let Some(property) = item
            .aspect_lock_property(effect)
            .filter(|property| property.is_editable(None))
        else {
            return false;
        };
        let ratio = if locked {
            let Some(ratio) = item.aspect_ratio(effect).or_else(|| {
                AspectRatio::from_value(item.property_values(effect)?.property(property.id())?)
            }) else {
                return false;
            };
            Some(ratio)
        } else {
            None
        };
        self.edit_project_if_changed(Some(HistoryKey::AspectRatioLock(id, effect)), |editor| {
            editor
                .active_document_mut()
                .set_item_aspect_ratio(id, effect, ratio)
        })
    }
}
