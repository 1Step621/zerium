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
        array: Option<usize>,
        tuple: Option<usize>,
    },
    Value,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct PropertyPath {
    owner: PropertyOwner,
    parameter_id: String,
    slot: PropertySlot,
}

impl PropertyPath {
    pub(crate) fn item_parameter(
        plugin_id: impl Into<String>,
        item_id: impl Into<String>,
        parameter_id: impl Into<String>,
    ) -> Self {
        Self {
            owner: PropertyOwner::Item {
                plugin_id: plugin_id.into(),
                item_id: item_id.into(),
            },
            parameter_id: parameter_id.into(),
            slot: PropertySlot::Value,
        }
    }

    pub(crate) fn effect_parameter(instance_id: u64, parameter_id: impl Into<String>) -> Self {
        Self {
            owner: PropertyOwner::Effect { instance_id },
            parameter_id: parameter_id.into(),
            slot: PropertySlot::Value,
        }
    }

    pub(crate) fn scene_parameter(scene_id: u64, parameter_id: impl Into<String>) -> Self {
        Self {
            owner: PropertyOwner::Scene { scene_id },
            parameter_id: parameter_id.into(),
            slot: PropertySlot::Value,
        }
    }

    pub(crate) fn scalar(&self, array: Option<usize>, tuple: Option<usize>) -> Self {
        self.with_slot(PropertySlot::Scalar { array, tuple })
    }

    fn with_slot(&self, slot: PropertySlot) -> Self {
        Self {
            owner: self.owner.clone(),
            parameter_id: self.parameter_id.clone(),
            slot,
        }
    }
}

impl fmt::Display for PropertyPath {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.owner {
            PropertyOwner::Item { plugin_id, item_id } => {
                write!(formatter, "{plugin_id}/{item_id}/{}", self.parameter_id)?;
            }
            PropertyOwner::Effect { instance_id } => {
                write!(formatter, "effect/{instance_id}/{}", self.parameter_id)?;
            }
            PropertyOwner::Scene { scene_id } => {
                write!(formatter, "scene/{scene_id}/{}", self.parameter_id)?;
            }
        }
        match self.slot {
            PropertySlot::Value => Ok(()),
            PropertySlot::Scalar { array, tuple } => {
                write!(formatter, "/scalar/{array:?}/{tuple:?}")
            }
        }
    }
}
