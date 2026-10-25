use crate::property::PropertyValue;
use crate::timeline::history::{EditImpact, HistoryKey};
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
        self.commit_item_edits(Some(key), [candidate])
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
        let Some(current) = item.authored_property(effect, schema) else {
            return false;
        };
        // Reset restores schema inheritance for every authored property.
        let inherit = matches!(edit, PropertyEdit::Reset)
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
        if let Some(ratio) = item.aspect_ratio(effect).filter(|_| !inherit).filter(|_| {
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
        if inherit
            && item.aspect_ratio(effect).is_some()
            && item
                .aspect_lock_property(effect)
                .is_some_and(|property| property.id() == schema.id())
        {
            candidate
                .item
                .set_aspect_ratio(effect, AspectRatio::from_value(&next));
        }
        candidate.set_property(
            effect,
            schema,
            (!inherit).then_some(next),
            self.frame_rate(),
        )
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
                AspectRatio::from_value(&item.property_overrides(effect)?.resolve(property))
            }) else {
                return false;
            };
            Some(ratio)
        } else {
            None
        };
        self.edit_project_if_changed(
            EditImpact::Values,
            Some(HistoryKey::AspectRatioLock(id, effect)),
            |editor| {
                editor
                    .active_document_mut()
                    .item_mut(id)
                    .expect("prepared item must exist")
                    .set_aspect_ratio(effect, ratio)
            },
        )
    }
}
