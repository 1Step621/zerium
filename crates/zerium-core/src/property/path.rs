//! Stable scalar coordinates shared by editing, animation, and scene bindings.
use serde::{Deserialize, Serialize};

use super::{PropertyElementId, PropertyValue, PropertyValues};

#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Hash, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PropertyPath {
    #[serde(rename = "property")]
    property_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    element_id: Option<PropertyElementId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    scalar_index: Option<usize>,
}

impl PropertyPath {
    pub fn new(
        property_id: impl Into<String>,
        element_id: Option<PropertyElementId>,
        scalar_index: Option<usize>,
    ) -> Self {
        Self {
            property_id: property_id.into(),
            element_id,
            scalar_index,
        }
    }

    pub fn property_id(&self) -> &str {
        &self.property_id
    }

    pub const fn element_id(&self) -> Option<PropertyElementId> {
        self.element_id
    }

    pub const fn scalar_index(&self) -> Option<usize> {
        self.scalar_index
    }

    pub fn value<'a>(&self, values: &'a PropertyValues) -> Option<&'a PropertyValue> {
        values
            .property(self.property_id())?
            .scalar(self.element_id, self.scalar_index)
    }

    pub fn value_mut<'a>(&self, values: &'a mut PropertyValues) -> Option<&'a mut PropertyValue> {
        values
            .property_mut(self.property_id())?
            .scalar_mut(self.element_id, self.scalar_index)
    }
}
