//! Runtime property values and checked value collections.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use super::{PropertyError, PropertyPath, schema::PropertySchema};
pub(crate) const MAX_STRING_BYTES: usize = 4_096;

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct PropertyElementId(pub(super) u64);

impl PropertyElementId {
    pub const fn is_valid(self) -> bool {
        self.0 != 0
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PropertyElement {
    pub(super) id: PropertyElementId,
    pub(super) value: PropertyValue,
}

impl PropertyElement {
    pub const fn element_id(&self) -> PropertyElementId {
        self.id
    }

    pub const fn value(&self) -> &PropertyValue {
        &self.value
    }

    pub const fn value_mut(&mut self) -> &mut PropertyValue {
        &mut self.value
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PropertyValue {
    File(Option<std::path::PathBuf>),
    F32(f32),
    I32(i32),
    U32(u32),
    Enum(u32),
    Bool(bool),
    Tuple(Vec<PropertyValue>),
    Color([f32; 4]),
    String(String),
    Array(Vec<PropertyElement>),
}

impl PropertyValue {
    pub fn as_f32(&self) -> Option<f32> {
        match self {
            Self::F32(value) => Some(*value),
            _ => None,
        }
    }

    pub fn as_u32(&self) -> Option<u32> {
        match self {
            Self::U32(value) => Some(*value),
            _ => None,
        }
    }

    pub fn as_enum(&self) -> Option<u32> {
        match self {
            Self::Enum(value) => Some(*value),
            _ => None,
        }
    }

    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Self::Bool(value) => Some(*value),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Self::String(value) => Some(value),
            _ => None,
        }
    }

    pub fn as_color(&self) -> Option<[f32; 4]> {
        match self {
            Self::Color(value) => Some(*value),
            _ => None,
        }
    }

    pub fn as_array(&self) -> Option<&[PropertyElement]> {
        match self {
            Self::Array(elements) => Some(elements),
            _ => None,
        }
    }

    pub fn scalar(
        &self,
        element_id: Option<PropertyElementId>,
        scalar_index: Option<usize>,
    ) -> Option<&Self> {
        self.element(element_id)?.scalar_at(scalar_index)
    }

    pub fn scalar_mut(
        &mut self,
        element_id: Option<PropertyElementId>,
        scalar_index: Option<usize>,
    ) -> Option<&mut Self> {
        self.element_mut(element_id)?.scalar_at_mut(scalar_index)
    }

    /// Replace a whole value, array element, or tuple scalar without changing siblings.
    pub fn replaced_at(
        &self,
        element_id: Option<PropertyElementId>,
        scalar_index: Option<usize>,
        value: Self,
    ) -> Option<Self> {
        let mut next = self.clone();
        let element = next.element_mut(element_id)?;
        let target = match scalar_index {
            Some(index) => element.scalar_at_mut(Some(index))?,
            None => element,
        };
        *target = value;
        Some(next)
    }

    pub fn file(&self) -> Option<&std::path::Path> {
        match self {
            Self::File(path) => path.as_deref(),
            _ => None,
        }
    }

    /// Leaf values with stable array element IDs and structural tuple indices.
    pub fn scalars(
        &self,
    ) -> impl Iterator<Item = (Option<PropertyElementId>, Option<usize>, &Self)> {
        let array = match self {
            Self::Array(elements) => Some(elements.as_slice()),
            _ => None,
        };
        array
            .is_none()
            .then_some((None, self))
            .into_iter()
            .chain(
                array
                    .into_iter()
                    .flatten()
                    .map(|element| (Some(element.element_id()), element.value())),
            )
            .flat_map(|(element_id, value)| {
                let (scalars, tuple) = match value {
                    Self::Tuple(values) => (values.as_slice(), true),
                    value => (std::slice::from_ref(value), false),
                };
                scalars
                    .iter()
                    .enumerate()
                    .map(move |(index, value)| (element_id, tuple.then_some(index), value))
            })
    }

    pub fn map_file_paths(&mut self, map: &mut impl FnMut(&std::path::Path) -> std::path::PathBuf) {
        match self {
            Self::File(Some(path)) => *path = map(path),
            Self::Tuple(values) => {
                for value in values {
                    value.map_file_paths(map);
                }
            }
            Self::Array(elements) => {
                for element in elements {
                    element.value_mut().map_file_paths(map);
                }
            }
            _ => {}
        }
    }

    pub fn scalar_at(&self, scalar_index: Option<usize>) -> Option<&Self> {
        match (self, scalar_index) {
            (Self::Tuple(values), Some(index)) => values.get(index),
            (Self::Tuple(_) | Self::Array(_), _) => None,
            (value, None) => Some(value),
            _ => None,
        }
    }

    pub fn scalar_at_mut(&mut self, scalar_index: Option<usize>) -> Option<&mut Self> {
        match (self, scalar_index) {
            (Self::Tuple(values), Some(index)) => values.get_mut(index),
            (Self::Tuple(_) | Self::Array(_), _) => None,
            (value, None) => Some(value),
            _ => None,
        }
    }

    /// Resolves one array element, or the value itself for a non-array target.
    pub fn element(&self, element_id: Option<PropertyElementId>) -> Option<&Self> {
        match (self, element_id) {
            (Self::Array(elements), Some(id)) => elements
                .iter()
                .find(|element| element.element_id() == id)
                .map(PropertyElement::value),
            (_, Some(_)) => None,
            (_, None) => Some(self),
        }
    }

    pub fn element_mut(&mut self, element_id: Option<PropertyElementId>) -> Option<&mut Self> {
        match element_id {
            Some(id) => match self {
                Self::Array(elements) => elements
                    .iter_mut()
                    .find(|element| element.element_id() == id)
                    .map(PropertyElement::value_mut),
                _ => None,
            },
            None => Some(self),
        }
    }

    pub fn element_index(&self, id: PropertyElementId) -> Option<usize> {
        let Self::Array(elements) = self else {
            return None;
        };
        elements
            .iter()
            .position(|element| element.element_id() == id)
    }

    pub fn f32_tuple<const N: usize>(values: [f32; N]) -> Self {
        Self::Tuple(values.into_iter().map(Self::F32).collect())
    }

    pub fn push_element(&mut self, value: PropertyValue) -> bool {
        let Self::Array(elements) = self else {
            return false;
        };
        let Some(id) = elements
            .iter()
            .map(|element| element.element_id().0)
            .max()
            .unwrap_or(0)
            .checked_add(1)
        else {
            return false;
        };
        elements.push(PropertyElement {
            id: PropertyElementId(id),
            value,
        });
        true
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct PropertyValues {
    values: HashMap<String, PropertyValue>,
}

impl PropertyValues {
    pub fn from_properties(properties: &[PropertySchema]) -> Self {
        Self {
            values: properties
                .iter()
                .map(|property| (property.id.clone(), property.default_value()))
                .collect(),
        }
    }

    pub fn property(&self, id: &str) -> Option<&PropertyValue> {
        self.values.get(id)
    }

    pub fn files(&self) -> impl Iterator<Item = (PropertyPath, &std::path::Path)> {
        self.iter().flat_map(|(id, value)| {
            value
                .scalars()
                .filter_map(move |(element_id, scalar_index, value)| {
                    value
                        .file()
                        .map(|path| (PropertyPath::new(id, element_id, scalar_index), path))
                })
        })
    }

    pub fn property_mut(&mut self, id: &str) -> Option<&mut PropertyValue> {
        self.values.get_mut(id)
    }

    pub fn remove(&mut self, id: &str) -> Option<PropertyValue> {
        self.values.remove(id)
    }

    pub fn iter(&self) -> impl Iterator<Item = (&str, &PropertyValue)> {
        self.values.iter().map(|(id, value)| (id.as_str(), value))
    }

    pub fn set(
        &mut self,
        property: &PropertySchema,
        value: PropertyValue,
    ) -> Result<bool, PropertyError> {
        if !property.accepts_value(&value) {
            return Err(PropertyError::invalid_definition(format!(
                "property '{}' value violates its schema contract",
                property.id
            )));
        }
        if self.property(&property.id) == Some(&value) {
            return Ok(false);
        }
        self.values.insert(property.id.clone(), value);
        Ok(true)
    }

    pub(crate) fn validate_for(
        &self,
        owner_kind: &str,
        owner_id: &str,
        properties: &[PropertySchema],
    ) -> Result<(), PropertyError> {
        if self.values.len() != properties.len() {
            return Err(PropertyError::invalid_definition(format!(
                "{owner_kind} '{owner_id}' property set is incomplete"
            )));
        }
        for property in properties {
            let value = self.values.get(property.id()).ok_or_else(|| {
                PropertyError::invalid_definition(format!(
                    "{owner_kind} '{owner_id}' is missing property '{}'",
                    property.id()
                ))
            })?;
            if !property.accepts_value(value) {
                return Err(PropertyError::invalid_definition(format!(
                    "{owner_kind} '{owner_id}' property '{}' does not match its schema contract",
                    property.id()
                )));
            }
        }
        Ok(())
    }
}

pub fn materialized_property_values<'a>(
    overrides: &PropertyValues,
    schema: impl IntoIterator<Item = &'a PropertySchema>,
) -> PropertyValues {
    let mut values = PropertyValues::default();
    for property in schema {
        let value = property.resolve_value(overrides.property(property.id()));
        values
            .set(property, value)
            .expect("constrained values preserve the property type");
    }
    values
}
