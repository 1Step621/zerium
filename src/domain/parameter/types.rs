//! Structural parameter contracts. Storage and editor projections live in their adapters.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use super::{
    value::{MAX_STRING_BYTES, ParameterValue},
    wire::TypeDefinition,
};

pub(super) const MAX_TUPLE_ELEMENTS: usize = 64;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "Vec<u32>", into = "Vec<u32>")]
pub(crate) struct EnumParameterType {
    values: Box<[u32]>,
}

impl EnumParameterType {
    pub(super) fn new(values: Vec<u32>) -> Result<Self, &'static str> {
        let mut seen = std::collections::HashSet::new();
        if values.is_empty() || !values.iter().all(|value| seen.insert(*value)) {
            return Err("enum values must be unique and non-empty");
        }
        Ok(Self {
            values: values.into(),
        })
    }

    pub(super) fn into_values(self) -> Box<[u32]> {
        self.values
    }

    pub(crate) fn values(&self) -> &[u32] {
        &self.values
    }

    pub(super) fn contains(&self, value: u32) -> bool {
        self.values.contains(&value)
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(try_from = "TypeDefinition", into = "TypeDefinition")]
pub(crate) enum ScalarParameterType {
    F32,
    I32,
    U32,
    Bool,
    Color,
    String,
    Enum(EnumParameterType),
}

impl ScalarParameterType {
    pub(crate) const fn is_interpolatable(&self) -> bool {
        matches!(self, Self::F32 | Self::I32 | Self::U32 | Self::Color)
    }

    pub(crate) fn allows(&self, value: &ParameterValue) -> bool {
        match (self, value) {
            (Self::F32, ParameterValue::F32(value)) => value.is_finite(),
            (Self::I32, ParameterValue::I32(_))
            | (Self::U32, ParameterValue::U32(_))
            | (Self::Bool, ParameterValue::Bool(_)) => true,
            (Self::String, ParameterValue::String(value)) => value.len() <= MAX_STRING_BYTES,
            (Self::Color, ParameterValue::Color(values)) => {
                values.iter().all(|value| value.is_finite())
            }
            (Self::Enum(ty), ParameterValue::Enum(value)) => ty.contains(*value),
            _ => false,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    try_from = "Vec<ScalarParameterType>",
    into = "Vec<ScalarParameterType>"
)]
pub(crate) struct TupleParameterType {
    elements: Box<[ScalarParameterType]>,
}

impl TupleParameterType {
    pub(super) fn into_elements(self) -> Box<[ScalarParameterType]> {
        self.elements
    }

    pub(crate) fn new(elements: impl Into<Box<[ScalarParameterType]>>) -> Option<Self> {
        let elements = elements.into();
        (2..=MAX_TUPLE_ELEMENTS)
            .contains(&elements.len())
            .then_some(Self { elements })
    }

    pub(crate) fn elements(&self) -> &[ScalarParameterType] {
        &self.elements
    }

    pub(crate) fn element_count(&self) -> usize {
        self.elements.len()
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(try_from = "TypeDefinition", into = "TypeDefinition")]
pub(crate) enum ParameterValueType {
    Scalar(ScalarParameterType),
    Tuple(TupleParameterType),
}

impl ParameterValueType {
    pub(crate) fn allows(&self, value: &ParameterValue) -> bool {
        match self {
            Self::Scalar(ty) => ty.allows(value),
            Self::Tuple(tuple) => {
                let ParameterValue::Tuple(values) = value else {
                    return false;
                };
                values.len() == tuple.element_count()
                    && values
                        .iter()
                        .zip(tuple.elements())
                        .all(|(value, ty)| ty.allows(value))
            }
        }
    }

    /// Scalars with their structural tuple index; a standalone scalar has no index.
    pub(crate) fn scalars(&self) -> impl Iterator<Item = (Option<usize>, &ScalarParameterType)> {
        let (elements, tuple) = match self {
            Self::Scalar(ty) => (std::slice::from_ref(ty), false),
            Self::Tuple(tuple) => (tuple.elements(), true),
        };
        elements
            .iter()
            .enumerate()
            .map(move |(index, ty)| (tuple.then_some(index), ty))
    }

    pub(crate) fn scalar_type(&self) -> Option<&ScalarParameterType> {
        match self {
            Self::Scalar(ty) => Some(ty),
            Self::Tuple(_) => None,
        }
    }

    /// Resolve one structural scalar; color remains one scalar.
    pub(crate) fn scalar_at(&self, element: Option<usize>) -> Option<&ScalarParameterType> {
        match (self, element) {
            (Self::Scalar(ty), None) => Some(ty),
            (Self::Tuple(tuple), Some(index)) => tuple.elements().get(index),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(try_from = "TypeDefinition", into = "TypeDefinition")]
pub(crate) enum ParameterType {
    Value(ParameterValueType),
    Array {
        element: ParameterValueType,
        min_items: u32,
        max_items: u32,
    },
}

impl ParameterType {
    pub(crate) fn allows(&self, value: &ParameterValue) -> bool {
        match self {
            Self::Value(ty) => ty.allows(value),
            Self::Array {
                element,
                min_items,
                max_items,
            } => {
                let ParameterValue::Array(values) = value else {
                    return false;
                };
                let mut ids = HashSet::with_capacity(values.len());
                values.len() >= *min_items as usize
                    && values.len() <= *max_items as usize
                    && values.iter().all(|array_element| {
                        array_element.id().is_valid()
                            && ids.insert(array_element.id())
                            && element.allows(array_element.value())
                    })
            }
        }
    }

    pub(crate) fn value_type(&self) -> Option<&ParameterValueType> {
        match self {
            Self::Value(ty) => Some(ty),
            Self::Array { .. } => None,
        }
    }

    pub(crate) fn scalar_type(&self) -> Option<&ScalarParameterType> {
        self.value_type()?.scalar_type()
    }

    pub(crate) fn array_element_type(&self) -> Option<&ParameterValueType> {
        match self {
            Self::Array { element, .. } => Some(element),
            _ => None,
        }
    }

    pub(crate) fn element_type(&self) -> &ParameterValueType {
        match self {
            Self::Value(element) | Self::Array { element, .. } => element,
        }
    }
}
