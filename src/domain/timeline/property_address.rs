use crate::domain::property::{PropertyElementId, PropertyPath, PropertyValue};

use super::ids::{EffectInstanceId, ItemId};

/// Identifies a property value independently of any inspector row or widget.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct PropertyAddress {
    pub(crate) item_id: ItemId,
    pub(crate) effect_id: Option<EffectInstanceId>,
    pub(crate) property_id: String,
    pub(crate) element_id: Option<PropertyElementId>,
    pub(crate) scalar_index: Option<usize>,
}

impl PropertyAddress {
    pub(crate) fn path(&self) -> PropertyPath {
        PropertyPath::new(&self.property_id, self.element_id, self.scalar_index)
    }

    pub(crate) fn value<'a>(&self, item: &'a super::TimelineItem) -> Option<&'a PropertyValue> {
        self.path().value(item.property_values(self.effect_id)?)
    }
}
