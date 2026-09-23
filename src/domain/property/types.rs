//! Structural property contracts. Storage and editor projections live in their adapters.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use super::value::{MAX_STRING_BYTES, PropertyValue};

pub(super) const MAX_TUPLE_ELEMENTS: usize = 64;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "Vec<u32>", into = "Vec<u32>")]
pub(crate) struct EnumPropertyType {
    values: Box<[u32]>,
}

impl EnumPropertyType {
    pub(super) fn new(values: Vec<u32>) -> Result<Self, &'static str> {
        let mut seen = std::collections::HashSet::new();
        if values.is_empty() || !values.iter().all(|value| seen.insert(*value)) {
            return Err("enum values must be unique and non-empty");
        }
        Ok(Self {
            values: values.into(),
        })
    }

    pub(crate) fn values(&self) -> &[u32] {
        &self.values
    }
}

impl TryFrom<Vec<u32>> for EnumPropertyType {
    type Error = &'static str;

    fn try_from(values: Vec<u32>) -> Result<Self, Self::Error> {
        Self::new(values)
    }
}

impl From<EnumPropertyType> for Vec<u32> {
    fn from(value: EnumPropertyType) -> Self {
        value.values.into()
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ScalarPropertyType {
    F32,
    I32,
    U32,
    Bool,
    Color,
    String,
    Enum(EnumPropertyType),
}

impl ScalarPropertyType {
    pub(crate) const fn is_interpolatable(&self) -> bool {
        matches!(self, Self::F32 | Self::I32 | Self::U32 | Self::Color)
    }

    pub(crate) fn allows(&self, value: &PropertyValue) -> bool {
        match (self, value) {
            (Self::F32, PropertyValue::F32(value)) => value.is_finite(),
            (Self::I32, PropertyValue::I32(_))
            | (Self::U32, PropertyValue::U32(_))
            | (Self::Bool, PropertyValue::Bool(_)) => true,
            (Self::String, PropertyValue::String(value)) => value.len() <= MAX_STRING_BYTES,
            (Self::Color, PropertyValue::Color(values)) => {
                values.iter().all(|value| value.is_finite())
            }
            (Self::Enum(ty), PropertyValue::Enum(value)) => ty.values().contains(value),
            _ => false,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "Vec<ScalarPropertyType>", into = "Vec<ScalarPropertyType>")]
pub(crate) struct TuplePropertyType {
    scalars: Box<[ScalarPropertyType]>,
}

impl TuplePropertyType {
    pub(crate) fn new(scalars: impl Into<Box<[ScalarPropertyType]>>) -> Option<Self> {
        let scalars = scalars.into();
        (2..=MAX_TUPLE_ELEMENTS)
            .contains(&scalars.len())
            .then_some(Self { scalars })
    }

    pub(crate) fn scalars(&self) -> &[ScalarPropertyType] {
        &self.scalars
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
        value.scalars.into()
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(untagged)]
pub(crate) enum PropertyValueType {
    Scalar(ScalarPropertyType),
    Tuple(TuplePropertyType),
}

impl PropertyValueType {
    pub(crate) fn allows(&self, value: &PropertyValue) -> bool {
        match self {
            Self::Scalar(ty) => ty.allows(value),
            Self::Tuple(tuple) => {
                let PropertyValue::Tuple(values) = value else {
                    return false;
                };
                values.len() == tuple.scalars().len()
                    && values
                        .iter()
                        .zip(tuple.scalars())
                        .all(|(value, ty)| ty.allows(value))
            }
        }
    }

    /// Scalars with their structural tuple index; a standalone scalar has no index.
    pub(crate) fn scalars(&self) -> impl Iterator<Item = (Option<usize>, &ScalarPropertyType)> {
        let (scalars, tuple) = match self {
            Self::Scalar(ty) => (std::slice::from_ref(ty), false),
            Self::Tuple(tuple) => (tuple.scalars(), true),
        };
        scalars
            .iter()
            .enumerate()
            .map(move |(index, ty)| (tuple.then_some(index), ty))
    }

    /// Resolve one structural scalar; color remains one scalar.
    pub(crate) fn scalar_at(&self, scalar_index: Option<usize>) -> Option<&ScalarPropertyType> {
        match (self, scalar_index) {
            (Self::Scalar(ty), None) => Some(ty),
            (Self::Tuple(tuple), Some(index)) => tuple.scalars().get(index),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum PropertyType {
    Value(PropertyValueType),
    Array {
        element_type: PropertyValueType,
        #[serde(default, skip_serializing_if = "is_zero")]
        min_items: u32,
        max_items: u32,
    },
}

fn is_zero(value: &u32) -> bool {
    *value == 0
}

impl PropertyType {
    pub(crate) fn allows(&self, value: &PropertyValue) -> bool {
        match self {
            Self::Value(ty) => ty.allows(value),
            Self::Array {
                element_type,
                min_items,
                max_items,
            } => {
                let PropertyValue::Array(values) = value else {
                    return false;
                };
                let mut ids = HashSet::with_capacity(values.len());
                values.len() >= *min_items as usize
                    && values.len() <= *max_items as usize
                    && values.iter().all(|element| {
                        element.element_id().is_valid()
                            && ids.insert(element.element_id())
                            && element_type.allows(element.value())
                    })
            }
        }
    }
}
