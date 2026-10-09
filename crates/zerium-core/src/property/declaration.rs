//! Authoring declarations use plain defaults; project values keep explicit type tags.
use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error as _};
use serde_json::{Map, Value};

use super::schema::{PropertyConfiguration, PropertyDefinition, ScalarSchema, ValueSchema};
use super::{
    EnumPropertyType, PropertyElement, PropertyElementId, PropertySchema, PropertyValue,
    ScalarPropertyType,
};

fn required<T: serde::de::DeserializeOwned>(
    fields: &mut Map<String, Value>,
    name: &str,
) -> Result<T, String> {
    let value = fields
        .remove(name)
        .ok_or_else(|| format!("missing field '{name}'"))?;
    serde_json::from_value(value).map_err(|error| error.to_string())
}

fn object(value: Value) -> Result<Map<String, Value>, String> {
    match value {
        Value::Object(fields) => Ok(fields),
        _ => Err("property declaration must be an object".into()),
    }
}

fn no_remaining_fields(fields: Map<String, Value>) -> Result<(), String> {
    if let Some(name) = fields.keys().next() {
        Err(format!("unknown field '{name}'"))
    } else {
        Ok(())
    }
}

impl ValueSchema {
    fn from_fields(mut fields: Map<String, Value>) -> Result<Self, String> {
        let kind: String = required(&mut fields, "type")?;
        if kind == "tuple" {
            let elements: Vec<Value> = required(&mut fields, "elements")?;
            no_remaining_fields(fields)?;
            if !(2..=super::types::MAX_TUPLE_ELEMENTS).contains(&elements.len()) {
                return Err("tuple must contain 2–64 scalars".into());
            }
            let elements = elements
                .into_iter()
                .map(|value| match Self::from_fields(object(value)?)? {
                    Self::Scalar(scalar) => Ok(scalar),
                    Self::Tuple(_) => Err("nested tuples are unsupported".into()),
                })
                .collect::<Result<_, String>>()?;
            return Ok(Self::Tuple(elements));
        }
        let ty = if kind == "enum" {
            ScalarPropertyType::Enum(
                EnumPropertyType::new(required(&mut fields, "variants")?).map_err(str::to_owned)?,
            )
        } else {
            serde_json::from_value(Value::String(kind)).map_err(|error| error.to_string())?
        };
        let default = required(&mut fields, "default")?;
        let configuration: PropertyConfiguration =
            serde_json::from_value(Value::Object(fields)).map_err(|error| error.to_string())?;
        let default = parse_scalar(&ty, default)?;
        Ok(Self::Scalar(ScalarSchema {
            ty,
            default,
            configuration,
        }))
    }

    fn to_fields(&self) -> Result<Map<String, Value>, serde_json::Error> {
        match self {
            Self::Tuple(elements) => Ok(Map::from_iter([
                ("type".into(), Value::String("tuple".into())),
                (
                    "elements".into(),
                    Value::Array(
                        elements
                            .iter()
                            .map(|scalar| scalar.to_fields().map(Value::Object))
                            .collect::<Result<_, _>>()?,
                    ),
                ),
            ])),
            Self::Scalar(scalar) => scalar.to_fields(),
        }
    }

    fn parse_value(&self, value: Value) -> Result<PropertyValue, String> {
        match self {
            Self::Scalar(scalar) => parse_scalar(&scalar.ty, value),
            Self::Tuple(elements) => {
                let Value::Array(values) = value else {
                    return Err("tuple default must be an array".into());
                };
                if values.len() != elements.len() {
                    return Err("tuple default length does not match its elements".into());
                }
                elements
                    .iter()
                    .zip(values)
                    .map(|(scalar, value)| parse_scalar(&scalar.ty, value))
                    .collect::<Result<_, _>>()
                    .map(PropertyValue::Tuple)
            }
        }
    }
}

impl ScalarSchema {
    fn to_fields(&self) -> Result<Map<String, Value>, serde_json::Error> {
        let mut fields: Map<String, Value> =
            serde_json::from_value(serde_json::to_value(&self.configuration)?)?;
        let kind = match &self.ty {
            ScalarPropertyType::Enum(enumeration) => {
                fields.insert(
                    "variants".into(),
                    serde_json::to_value(enumeration.variants())?,
                );
                Value::String("enum".into())
            }
            ty => serde_json::to_value(ty)?,
        };
        fields.insert("type".into(), kind);
        fields.insert("default".into(), plain_value(&self.default)?);
        Ok(fields)
    }
}

fn parse_scalar(ty: &ScalarPropertyType, value: Value) -> Result<PropertyValue, String> {
    let parsed = match ty {
        ScalarPropertyType::File => serde_json::from_value(value).map(PropertyValue::File),
        ScalarPropertyType::F32 => serde_json::from_value(value).map(PropertyValue::F32),
        ScalarPropertyType::I32 => serde_json::from_value(value).map(PropertyValue::I32),
        ScalarPropertyType::U32 => serde_json::from_value(value).map(PropertyValue::U32),
        ScalarPropertyType::Bool => serde_json::from_value(value).map(PropertyValue::Bool),
        ScalarPropertyType::Color => serde_json::from_value(value).map(PropertyValue::Color),
        ScalarPropertyType::String => serde_json::from_value(value).map(PropertyValue::String),
        ScalarPropertyType::Enum(_) => serde_json::from_value(value).map(PropertyValue::Enum),
    };
    parsed.map_err(|error| error.to_string())
}

fn plain_value(value: &PropertyValue) -> Result<Value, serde_json::Error> {
    match value {
        PropertyValue::File(value) => serde_json::to_value(value),
        PropertyValue::F32(value) => serde_json::to_value(value),
        PropertyValue::I32(value) => serde_json::to_value(value),
        PropertyValue::U32(value) | PropertyValue::Enum(value) => serde_json::to_value(value),
        PropertyValue::Bool(value) => serde_json::to_value(value),
        PropertyValue::Color(value) => serde_json::to_value(value),
        PropertyValue::String(value) => serde_json::to_value(value),
        PropertyValue::Tuple(values) => values
            .iter()
            .map(plain_value)
            .collect::<Result<_, _>>()
            .map(Value::Array),
        PropertyValue::Array(values) => values
            .iter()
            .map(|element| plain_value(element.value()))
            .collect::<Result<_, _>>()
            .map(Value::Array),
    }
}

impl<'de> Deserialize<'de> for PropertySchema {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let mut fields = Map::deserialize(deserializer)?;
        let id = required(&mut fields, "id").map_err(D::Error::custom)?;
        let label = required(&mut fields, "label").map_err(D::Error::custom)?;
        let definition = if fields.get("type").and_then(Value::as_str) == Some("array") {
            fields.remove("type");
            let element = ValueSchema::from_fields(
                object(required(&mut fields, "element").map_err(D::Error::custom)?)
                    .map_err(D::Error::custom)?,
            )
            .map_err(D::Error::custom)?;
            let min_items = fields
                .remove("min_items")
                .map(serde_json::from_value)
                .transpose()
                .map_err(D::Error::custom)?
                .unwrap_or(0);
            let max_items = required(&mut fields, "max_items").map_err(D::Error::custom)?;
            let values: Vec<Value> = required(&mut fields, "default").map_err(D::Error::custom)?;
            no_remaining_fields(fields).map_err(D::Error::custom)?;
            let default = values
                .into_iter()
                .enumerate()
                .map(|(index, value)| {
                    Ok(PropertyElement {
                        id: PropertyElementId(index as u64 + 1),
                        value: element.parse_value(value)?,
                    })
                })
                .collect::<Result<_, String>>()
                .map_err(D::Error::custom)?;
            PropertyDefinition::Array {
                element,
                min_items,
                max_items,
                default,
            }
        } else {
            PropertyDefinition::Value(ValueSchema::from_fields(fields).map_err(D::Error::custom)?)
        };
        let schema = Self {
            id,
            label,
            definition,
        };
        schema
            .validate("schema", schema.id())
            .map_err(D::Error::custom)?;
        Ok(schema)
    }
}

impl Serialize for PropertySchema {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.validate("schema", self.id())
            .map_err(serde::ser::Error::custom)?;
        let mut fields = match &self.definition {
            PropertyDefinition::Value(value) => {
                value.to_fields().map_err(serde::ser::Error::custom)?
            }
            PropertyDefinition::Array {
                element,
                min_items,
                max_items,
                default,
            } => Map::from_iter([
                ("type".into(), Value::String("array".into())),
                (
                    "element".into(),
                    Value::Object(element.to_fields().map_err(serde::ser::Error::custom)?),
                ),
                ("min_items".into(), Value::from(*min_items)),
                ("max_items".into(), Value::from(*max_items)),
                (
                    "default".into(),
                    Value::Array(
                        default
                            .iter()
                            .map(|element| plain_value(element.value()))
                            .collect::<Result<_, _>>()
                            .map_err(serde::ser::Error::custom)?,
                    ),
                ),
            ]),
        };
        fields.insert("id".into(), Value::String(self.id.clone()));
        fields.insert(
            "label".into(),
            serde_json::to_value(&self.label).map_err(serde::ser::Error::custom)?,
        );
        fields.serialize(serializer)
    }
}
