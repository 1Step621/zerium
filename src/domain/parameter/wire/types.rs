//! Externally tagged JSON representations for structural parameter types.
use super::super::types::{
    EnumParameterType, MAX_TUPLE_ELEMENTS, ParameterType, ParameterValueType, ScalarParameterType,
    TupleParameterType,
};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub(in crate::domain::parameter) enum TypeDefinition {
    F32,
    I32,
    U32,
    Bool,
    Color,
    String,
    Enum(EnumParameterType),
    Tuple(TupleParameterType),
    Array {
        #[serde(rename = "type")]
        element: ParameterValueType,
        #[serde(default, skip_serializing_if = "is_zero")]
        min_items: u32,
        max_items: u32,
    },
}

fn is_zero(value: &u32) -> bool {
    *value == 0
}

impl TryFrom<Vec<u32>> for EnumParameterType {
    type Error = &'static str;
    fn try_from(values: Vec<u32>) -> Result<Self, Self::Error> {
        Self::new(values)
    }
}

impl From<EnumParameterType> for Vec<u32> {
    fn from(value: EnumParameterType) -> Self {
        value.into_values().into()
    }
}

impl TryFrom<Vec<ScalarParameterType>> for TupleParameterType {
    type Error = String;
    fn try_from(elements: Vec<ScalarParameterType>) -> Result<Self, Self::Error> {
        Self::new(elements).ok_or_else(|| {
            format!("tuple must contain between 2 and {MAX_TUPLE_ELEMENTS} elements")
        })
    }
}

impl From<TupleParameterType> for Vec<ScalarParameterType> {
    fn from(value: TupleParameterType) -> Self {
        value.into_elements().into()
    }
}

impl TryFrom<TypeDefinition> for ScalarParameterType {
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

impl From<ScalarParameterType> for TypeDefinition {
    fn from(value: ScalarParameterType) -> Self {
        match value {
            ScalarParameterType::F32 => Self::F32,
            ScalarParameterType::I32 => Self::I32,
            ScalarParameterType::U32 => Self::U32,
            ScalarParameterType::Bool => Self::Bool,
            ScalarParameterType::Color => Self::Color,
            ScalarParameterType::String => Self::String,
            ScalarParameterType::Enum(value) => Self::Enum(value),
        }
    }
}

impl TryFrom<TypeDefinition> for ParameterValueType {
    type Error = &'static str;
    fn try_from(value: TypeDefinition) -> Result<Self, Self::Error> {
        match value {
            TypeDefinition::Tuple(tuple) => Ok(Self::Tuple(tuple)),
            TypeDefinition::Array { .. } => Err("nested arrays are not supported"),
            value => ScalarParameterType::try_from(value).map(Self::Scalar),
        }
    }
}

impl From<ParameterValueType> for TypeDefinition {
    fn from(value: ParameterValueType) -> Self {
        match value {
            ParameterValueType::Scalar(value) => value.into(),
            ParameterValueType::Tuple(tuple) => Self::Tuple(tuple),
        }
    }
}

impl TryFrom<TypeDefinition> for ParameterType {
    type Error = &'static str;
    fn try_from(value: TypeDefinition) -> Result<Self, Self::Error> {
        match value {
            TypeDefinition::Array {
                element,
                min_items,
                max_items,
            } => {
                if max_items == 0 || max_items > 1_000_000 || min_items > max_items {
                    return Err(
                        "array bounds must satisfy 0 <= min_items <= max_items <= 1000000, with max_items > 0",
                    );
                }
                Ok(Self::Array {
                    element,
                    min_items,
                    max_items,
                })
            }
            value => ParameterValueType::try_from(value).map(Self::Value),
        }
    }
}

impl From<ParameterType> for TypeDefinition {
    fn from(value: ParameterType) -> Self {
        match value {
            ParameterType::Value(value) => value.into(),
            ParameterType::Array {
                element,
                min_items,
                max_items,
            } => Self::Array {
                element,
                min_items,
                max_items,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn externally_tagged_types_round_trip() {
        for definition in [
            json!("f32"),
            json!("i32"),
            json!("u32"),
            json!("bool"),
            json!("color"),
            json!("string"),
            json!({"enum":[0,7]}),
            json!({"tuple":["f32",{"enum":[0,7]},"color"]}),
            json!({"array":{"type":"f32","max_items":1024}}),
            json!({"array":{"type":{"tuple":["f32","i32"]},"min_items":3,"max_items":1024}}),
        ] {
            let ty: ParameterType = serde_json::from_value(definition.clone()).unwrap();
            assert_eq!(serde_json::to_value(ty).unwrap(), definition);
        }
        let ty: ParameterType = serde_json::from_value(json!({
            "array":{"type":"f32","min_items":0,"max_items":4}
        }))
        .unwrap();
        assert_eq!(
            serde_json::to_value(ty).unwrap(),
            json!({
                "array":{"type":"f32","max_items":4}
            })
        );
    }

    #[test]
    fn rejects_invalid_tags_payloads_and_container_nesting() {
        for definition in [
            json!({"array":"f32","max_items":1024}),
            json!({"array":{"type":"f32","max_items":0}}),
            json!({"array":{"type":"f32","min_items":3,"max_items":2}}),
            json!({"array":{"type":"f32","max_items":1000001}}),
            json!({"array":{"type":"f32"}}),
            json!({"array":{"type":"f32","max_items":2,"extra":true}}),
            json!({"array":{"type":{"array":{"type":"f32","max_items":2}},"max_items":2}}),
            json!({"tuple":[{"tuple":["f32","f32"]},"f32"]}),
            json!({"tuple":["f32"]}),
            json!({"enum":[]}),
            json!({"enum":[1,1]}),
            json!({"tuple":["f32","f32"],"enum":[1]}),
            json!({"unknown":{}}),
        ] {
            assert!(
                serde_json::from_value::<ParameterType>(definition.clone()).is_err(),
                "{definition}"
            );
        }
    }

    #[test]
    fn builtin_plugin_uses_the_new_array_format() {
        let manifest = include_str!("../../../../plugins/zerium.builtin/plugin.json");
        crate::domain::plugin::Plugin::from_loader(manifest, |_| Ok("test shader".into())).unwrap();
    }
}
