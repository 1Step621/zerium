//! JSON representations and conversions for property schemas and types.

use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error as _};
use serde_json::Value;

use super::{
    PropertyConstraints, PropertyScalarSchema, PropertySchema, PropertyUi, PropertyValue,
    types::{
        EnumPropertyType, MAX_TUPLE_ELEMENTS, PropertyType, PropertyValueType, ScalarPropertyType,
        TuplePropertyType,
    },
};

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct PropertySchemaDefinition {
    id: String,
    label: String,
    #[serde(rename = "type")]
    ty: PropertyType,
    default: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    editable: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    animatable: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    scalars: Option<Vec<PropertyScalarSchema>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    scene_bindable: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    constraints: Option<PropertyConstraints>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    ui: Option<PropertyUi>,
}

impl<'de> Deserialize<'de> for PropertySchema {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let definition = PropertySchemaDefinition::deserialize(deserializer)?;
        let default = PropertyValue::from_json(&definition.default, &definition.ty)
            .ok_or_else(|| D::Error::custom("property default does not match its type"))?;
        let scene_bindable = definition.scene_bindable.unwrap_or(true);
        let value_type = match &definition.ty {
            PropertyType::Value(value_type)
            | PropertyType::Array {
                element_type: value_type,
                ..
            } => value_type,
        };
        let scalars = match value_type {
            PropertyValueType::Scalar(_) => {
                if definition.scalars.is_some() {
                    return Err(D::Error::custom(
                        "property scalars requires a tuple value type",
                    ));
                }
                vec![PropertyScalarSchema {
                    editable: definition.editable.unwrap_or(true),
                    animatable: definition.animatable.unwrap_or(false),
                    constraints: definition.constraints.unwrap_or_default(),
                    ui: definition.ui.unwrap_or_default(),
                }]
            }
            PropertyValueType::Tuple(tuple) => {
                if definition.editable.is_some()
                    || definition.animatable.is_some()
                    || definition.constraints.is_some()
                    || definition.ui.is_some()
                {
                    return Err(D::Error::custom(
                        "tuple metadata must be specified in property scalars",
                    ));
                }
                let scalars = definition
                    .scalars
                    .ok_or_else(|| D::Error::custom("tuple properties require scalars"))?;
                if scalars.len() != tuple.scalar_count() {
                    return Err(D::Error::custom(
                        "property scalars must match the tuple length",
                    ));
                }
                scalars
            }
        };

        Ok(Self {
            id: definition.id,
            label: definition.label,
            ty: definition.ty,
            default,
            scalars,
            scene_bindable,
        })
    }
}

impl Serialize for PropertySchema {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let is_scalar = self.scalars.len() == 1;
        PropertySchemaDefinition {
            id: self.id.clone(),
            label: self.label.clone(),
            ty: self.ty.clone(),
            default: self.default.to_json_value(),
            editable: is_scalar
                .then_some(self.scalars[0].editable)
                .filter(|editable| !*editable),
            animatable: is_scalar
                .then_some(self.scalars[0].animatable)
                .filter(|animatable| *animatable),
            scalars: (!is_scalar).then(|| self.scalars.clone()),
            scene_bindable: (!self.scene_bindable).then_some(false),
            constraints: is_scalar
                .then_some(self.scalars[0].constraints.clone())
                .filter(|constraints| !constraints.is_default()),
            ui: is_scalar
                .then_some(self.scalars[0].ui.clone())
                .filter(|ui| !ui.is_default()),
        }
        .serialize(serializer)
    }
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub(in crate::domain::property) enum TypeDefinition {
    F32,
    I32,
    U32,
    Bool,
    Color,
    String,
    Enum(EnumPropertyType),
    Tuple(TuplePropertyType),
    Array {
        #[serde(rename = "type")]
        element_type: PropertyValueType,
        #[serde(default, skip_serializing_if = "is_zero")]
        min_items: u32,
        max_items: u32,
    },
}

fn is_zero(value: &u32) -> bool {
    *value == 0
}

impl TryFrom<Vec<u32>> for EnumPropertyType {
    type Error = &'static str;
    fn try_from(values: Vec<u32>) -> Result<Self, Self::Error> {
        Self::new(values)
    }
}

impl From<EnumPropertyType> for Vec<u32> {
    fn from(value: EnumPropertyType) -> Self {
        value.into_values().into()
    }
}

impl TryFrom<Vec<ScalarPropertyType>> for TuplePropertyType {
    type Error = String;
    fn try_from(scalars: Vec<ScalarPropertyType>) -> Result<Self, Self::Error> {
        Self::new(scalars).ok_or_else(|| {
            format!("tuple must contain between 2 and {MAX_TUPLE_ELEMENTS} elements")
        })
    }
}

impl From<TuplePropertyType> for Vec<ScalarPropertyType> {
    fn from(value: TuplePropertyType) -> Self {
        value.into_scalars().into()
    }
}

impl TryFrom<TypeDefinition> for ScalarPropertyType {
    type Error = &'static str;
    fn try_from(value: TypeDefinition) -> Result<Self, Self::Error> {
        match value {
            TypeDefinition::F32 => Ok(Self::F32),
            TypeDefinition::I32 => Ok(Self::I32),
            TypeDefinition::U32 => Ok(Self::U32),
            TypeDefinition::Bool => Ok(Self::Bool),
            TypeDefinition::Color => Ok(Self::Color),
            TypeDefinition::String => Ok(Self::String),
            TypeDefinition::Enum(value) => Ok(Self::Enum(value)),
            TypeDefinition::Tuple(_) | TypeDefinition::Array { .. } => {
                Err("expected a scalar type")
            }
        }
    }
}

impl From<ScalarPropertyType> for TypeDefinition {
    fn from(value: ScalarPropertyType) -> Self {
        match value {
            ScalarPropertyType::F32 => Self::F32,
            ScalarPropertyType::I32 => Self::I32,
            ScalarPropertyType::U32 => Self::U32,
            ScalarPropertyType::Bool => Self::Bool,
            ScalarPropertyType::Color => Self::Color,
            ScalarPropertyType::String => Self::String,
            ScalarPropertyType::Enum(value) => Self::Enum(value),
        }
    }
}

impl TryFrom<TypeDefinition> for PropertyValueType {
    type Error = &'static str;
    fn try_from(value: TypeDefinition) -> Result<Self, Self::Error> {
        match value {
            TypeDefinition::Tuple(tuple) => Ok(Self::Tuple(tuple)),
            TypeDefinition::Array { .. } => Err("nested arrays are not supported"),
            value => ScalarPropertyType::try_from(value).map(Self::Scalar),
        }
    }
}

impl From<PropertyValueType> for TypeDefinition {
    fn from(value: PropertyValueType) -> Self {
        match value {
            PropertyValueType::Scalar(value) => value.into(),
            PropertyValueType::Tuple(tuple) => Self::Tuple(tuple),
        }
    }
}

impl TryFrom<TypeDefinition> for PropertyType {
    type Error = &'static str;
    fn try_from(value: TypeDefinition) -> Result<Self, Self::Error> {
        match value {
            TypeDefinition::Array {
                element_type,
                min_items,
                max_items,
            } => {
                if max_items == 0 || max_items > 1_000_000 || min_items > max_items {
                    return Err(
                        "array bounds must satisfy 0 <= min_items <= max_items <= 1000000, with max_items > 0",
                    );
                }
                Ok(Self::Array {
                    element_type,
                    min_items,
                    max_items,
                })
            }
            value => PropertyValueType::try_from(value).map(Self::Value),
        }
    }
}

impl From<PropertyType> for TypeDefinition {
    fn from(value: PropertyType) -> Self {
        match value {
            PropertyType::Value(value) => value.into(),
            PropertyType::Array {
                element_type,
                min_items,
                max_items,
            } => Self::Array {
                element_type,
                min_items,
                max_items,
            },
        }
    }
}
