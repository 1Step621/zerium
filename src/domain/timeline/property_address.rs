use crate::domain::property::PropertyElementId;

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
