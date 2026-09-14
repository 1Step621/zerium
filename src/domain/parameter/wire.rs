//! JSON representations and conversions for parameter schemas and types.

use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error as _};
use serde_json::Value;

use super::{
    ParameterAnimatable, ParameterConstraints, ParameterEditable, ParameterSchema, ParameterUi,
    ParameterValue,
    types::{
        EnumParameterType, MAX_TUPLE_ELEMENTS, ParameterType, ParameterValueType,
        ScalarParameterType, TupleParameterType,
    },
};

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct AnimatableElements {
    elements: Vec<bool>,
}

#[derive(Deserialize, Serialize)]
#[serde(untagged)]
enum AnimatableWireDefinition {
    Scalar(bool),
    Tuple(AnimatableElements),
}

impl Default for AnimatableWireDefinition {
    fn default() -> Self {
        Self::Scalar(false)
    }
}

impl AnimatableWireDefinition {
    fn into_parameter(self) -> ParameterAnimatable {
        match self {
            Self::Scalar(enabled) => ParameterAnimatable::Scalar(enabled),
            Self::Tuple(elements) => ParameterAnimatable::Tuple(elements.elements),
        }
    }
}

impl From<&ParameterAnimatable> for AnimatableWireDefinition {
    fn from(value: &ParameterAnimatable) -> Self {
        match value {
            ParameterAnimatable::Scalar(enabled) => Self::Scalar(*enabled),
            ParameterAnimatable::Tuple(elements) => Self::Tuple(AnimatableElements {
                elements: elements.clone(),
            }),
        }
    }
}

type EditableElements = AnimatableElements;

#[derive(Deserialize, Serialize)]
#[serde(untagged)]
enum EditableWireDefinition {
    Scalar(bool),
    Tuple(EditableElements),
}

impl Default for EditableWireDefinition {
    fn default() -> Self {
        Self::Scalar(true)
    }
}

impl EditableWireDefinition {
    fn into_parameter(self) -> ParameterEditable {
        match self {
            Self::Scalar(enabled) => ParameterEditable::Scalar(enabled),
            Self::Tuple(elements) => ParameterEditable::Tuple(elements.elements),
        }
    }
}

impl From<&ParameterEditable> for EditableWireDefinition {
    fn from(value: &ParameterEditable) -> Self {
        match value {
            ParameterEditable::Scalar(enabled) => Self::Scalar(*enabled),
            ParameterEditable::Tuple(elements) => Self::Tuple(EditableElements {
                elements: elements.clone(),
            }),
        }
    }
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ParameterSchemaDefinition {
    id: String,
    label: String,
    #[serde(rename = "type")]
    ty: ParameterType,
    default: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    editable: Option<EditableWireDefinition>,
    #[serde(default)]
    animatable: AnimatableWireDefinition,
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
        let editable = definition.editable.unwrap_or_default().into_parameter();
        let animatable = definition.animatable.into_parameter();

        Ok(Self {
            id: definition.id,
            label: definition.label,
            ty: definition.ty,
            default,
            editable,
            animatable,
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
            editable: match &self.editable {
                ParameterEditable::Scalar(true) => None,
                editable => Some(editable.into()),
            },
            animatable: (&self.animatable).into(),
            scene_bindable: (!self.scene_bindable).then_some(false),
            constraints: self.constraints.clone(),
            ui: self.ui.clone(),
        }
        .serialize(serializer)
    }
}

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
