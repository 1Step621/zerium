//! Wire-format conversion for parameter schemas.

use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error as _};
use serde_json::Value;

use super::super::{ParameterConstraints, ParameterSchema, ParameterUi};
use crate::domain::parameter::{ParameterType, ParameterValue};

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ParameterSchemaDefinition {
    id: String,
    label: String,
    #[serde(rename = "type")]
    ty: ParameterType,
    default: Value,
    #[serde(default)]
    animatable: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    scene_bindable: Option<bool>,
    #[serde(default)]
    constraints: ParameterConstraints,
    #[serde(default)]
    ui: ParameterUi,
}

impl<'de> Deserialize<'de> for ParameterSchema {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let definition = ParameterSchemaDefinition::deserialize(deserializer)?;
        let default = ParameterValue::from_json(&definition.default, &definition.ty)
            .ok_or_else(|| D::Error::custom("parameter default does not match its type"))?;
        let scene_bindable = definition.scene_bindable.unwrap_or(true);

        Ok(Self {
            id: definition.id,
            label: definition.label,
            ty: definition.ty,
            default,
            animatable: definition.animatable,
            scene_bindable,
            constraints: definition.constraints,
            ui: definition.ui,
        })
    }
}

impl Serialize for ParameterSchema {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        ParameterSchemaDefinition {
            id: self.id.clone(),
            label: self.label.clone(),
            ty: self.ty.clone(),
            default: self.default.to_json_value(),
            animatable: self.animatable,
            scene_bindable: (!self.scene_bindable).then_some(false),
            constraints: self.constraints.clone(),
            ui: self.ui.clone(),
        }
        .serialize(serializer)
    }
}
