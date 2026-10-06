use crate::property::{PropertyElementId, PropertyPath, PropertyValue};

use super::ids::{EffectInstanceId, ItemId};

/// Identifies a property value independently of any inspector row or widget.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct PropertyAddress {
    pub item_id: ItemId,
    pub effect_id: Option<EffectInstanceId>,
    pub property_id: String,
    pub element_id: Option<PropertyElementId>,
    pub scalar_index: Option<usize>,
}

impl PropertyAddress {
    pub fn path(&self) -> PropertyPath {
        PropertyPath::new(&self.property_id, self.element_id, self.scalar_index)
    }

    pub fn schema<'a>(
        &self,
        editor: &'a super::TimelineEditor,
    ) -> Option<&'a crate::property::PropertySchema> {
        let item = editor.item(self.item_id)?;
        let owner = self.effect_id.map_or(
            super::SceneBindingOwner::Item,
            super::SceneBindingOwner::Effect,
        );
        super::scene::resolve_property_schema(
            &editor.project().scenes,
            item,
            owner,
            &self.property_id,
        )
    }

    pub fn value<'a>(&self, item: &'a super::TimelineItem) -> Option<&'a PropertyValue> {
        item.property_values(self.effect_id)?
            .property(&self.property_id)?
            .scalar(self.element_id, self.scalar_index)
    }
}
