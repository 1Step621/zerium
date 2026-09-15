use std::fmt;

use crate::domain::property::PropertyElementId;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum PropertyOwner {
    Detached,
    Item { plugin_id: String, item_id: String },
    Effect { instance_id: u64 },
    Scene { scene_id: u64 },
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum PropertySlot {
    Scalar {
        element_id: Option<PropertyElementId>,
        element_index: Option<usize>,
        scalar_index: Option<usize>,
    },
    Value,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct InspectorPath {
    owner: PropertyOwner,
    property_id: String,
    slot: PropertySlot,
}

impl InspectorPath {
    pub(crate) fn item_property(
        plugin_id: impl Into<String>,
        item_id: impl Into<String>,
        property_id: impl Into<String>,
    ) -> Self {
        Self {
            owner: PropertyOwner::Item {
                plugin_id: plugin_id.into(),
                item_id: item_id.into(),
            },
            property_id: property_id.into(),
            slot: PropertySlot::Value,
        }
    }

    pub(crate) fn effect_property(instance_id: u64, property_id: impl Into<String>) -> Self {
        Self {
            owner: PropertyOwner::Effect { instance_id },
            property_id: property_id.into(),
            slot: PropertySlot::Value,
        }
    }

    pub(crate) fn scene_property(scene_id: u64, property_id: impl Into<String>) -> Self {
        Self {
            owner: PropertyOwner::Scene { scene_id },
            property_id: property_id.into(),
            slot: PropertySlot::Value,
        }
    }

    pub(crate) fn new(element_id: Option<PropertyElementId>, scalar_index: Option<usize>) -> Self {
        Self {
            owner: PropertyOwner::Detached,
            property_id: String::new(),
            slot: PropertySlot::Scalar {
                element_id,
                element_index: None,
                scalar_index,
            },
        }
    }

    pub(crate) fn scalar(&self, element_index: Option<usize>, scalar_index: Option<usize>) -> Self {
        self.with_slot(PropertySlot::Scalar {
            element_id: None,
            element_index,
            scalar_index,
        })
    }

    pub(crate) fn element_id(&self) -> Option<PropertyElementId> {
        match self.slot {
            PropertySlot::Scalar { element_id, .. } => element_id,
            PropertySlot::Value => None,
        }
    }

    pub(crate) fn scalar_index(&self) -> Option<usize> {
        match self.slot {
            PropertySlot::Scalar { scalar_index, .. } => scalar_index,
            PropertySlot::Value => None,
        }
    }

    fn with_slot(&self, slot: PropertySlot) -> Self {
        Self {
            owner: self.owner.clone(),
            property_id: self.property_id.clone(),
            slot,
        }
    }
}

impl fmt::Display for InspectorPath {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.owner {
            PropertyOwner::Item { plugin_id, item_id } => {
                write!(formatter, "{plugin_id}/{item_id}/{}", self.property_id)?;
            }
            PropertyOwner::Detached => {
                write!(formatter, "<detached>/{}", self.property_id)?;
            }
            PropertyOwner::Effect { instance_id } => {
                write!(formatter, "effect/{instance_id}/{}", self.property_id)?;
            }
            PropertyOwner::Scene { scene_id } => {
                write!(formatter, "scene/{scene_id}/{}", self.property_id)?;
            }
        }
        match self.slot {
            PropertySlot::Value => Ok(()),
            PropertySlot::Scalar {
                element_id,
                element_index,
                scalar_index,
            } => {
                write!(
                    formatter,
                    "/scalar/{element_id:?}/{element_index:?}/{scalar_index:?}"
                )
            }
        }
    }
}
