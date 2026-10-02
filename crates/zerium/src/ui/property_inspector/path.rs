use std::fmt;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum PropertyOwner {
    Item { plugin_id: String, item_id: String },
    Effect { instance_id: u64 },
    Scene { scene_id: u64 },
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum PropertySlot {
    Scalar {
        element_index: Option<usize>,
        scalar_index: Option<usize>,
    },
    Value,
}

/// Identifies an inspector row or widget; editable property coordinates live in PropertyTarget.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(super) struct InspectorPath {
    owner: PropertyOwner,
    property_id: String,
    slot: PropertySlot,
}

impl InspectorPath {
    pub(super) fn item_property(
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

    pub(super) fn effect_property(instance_id: u64, property_id: impl Into<String>) -> Self {
        Self {
            owner: PropertyOwner::Effect { instance_id },
            property_id: property_id.into(),
            slot: PropertySlot::Value,
        }
    }

    pub(super) fn scene_property(scene_id: u64, property_id: impl Into<String>) -> Self {
        Self {
            owner: PropertyOwner::Scene { scene_id },
            property_id: property_id.into(),
            slot: PropertySlot::Value,
        }
    }

    pub(super) fn scalar(&self, element_index: Option<usize>, scalar_index: Option<usize>) -> Self {
        self.with_slot(PropertySlot::Scalar {
            element_index,
            scalar_index,
        })
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
                element_index,
                scalar_index,
            } => {
                write!(formatter, "/scalar/{element_index:?}/{scalar_index:?}")
            }
        }
    }
}
