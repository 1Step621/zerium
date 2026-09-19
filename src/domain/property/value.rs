//! Runtime property values and checked value collections.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{
    PropertyError,
    schema::PropertySchema,
    types::{PropertyType, PropertyValueType, ScalarPropertyType},
};
pub(in crate::domain) const MAX_STRING_BYTES: usize = 4_096;

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub(crate) struct PropertyElementId(u64);

impl PropertyElementId {
    pub(crate) const fn is_valid(self) -> bool {
        self.0 != 0
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PropertyElement {
    id: PropertyElementId,
    value: PropertyValue,
}

impl PropertyElement {
    pub(crate) const fn element_id(&self) -> PropertyElementId {
        self.id
    }

    pub(crate) const fn value(&self) -> &PropertyValue {
        &self.value
    }

    pub(crate) const fn value_mut(&mut self) -> &mut PropertyValue {
        &mut self.value
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
pub(crate) enum PropertyValue {
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
    pub(crate) fn scalar_at(&self, scalar_index: Option<usize>) -> Option<&Self> {
        match (self, scalar_index) {
            (Self::Tuple(values), Some(index)) => values.get(index),
            (Self::Tuple(_) | Self::Array(_), _) => None,
            (value, None) => Some(value),
            _ => None,
        }
    }

    pub(crate) fn scalar_at_mut(&mut self, scalar_index: Option<usize>) -> Option<&mut Self> {
        match (self, scalar_index) {
            (Self::Tuple(values), Some(index)) => values.get_mut(index),
            (Self::Tuple(_) | Self::Array(_), _) => None,
            (value, None) => Some(value),
            _ => None,
        }
    }

    /// Resolves one array element, or the value itself for a non-array target.
    pub(crate) fn element(&self, element_id: Option<PropertyElementId>) -> Option<&Self> {
        match (self, element_id) {
            (Self::Array(elements), Some(id)) => elements
                .iter()
                .find(|element| element.element_id() == id)
                .map(PropertyElement::value),
            (Self::Array(_), None) => Some(self),
            (_, Some(_)) => None,
            (_, None) => Some(self),
        }
    }

    pub(crate) fn element_mut(
        &mut self,
        element_id: Option<PropertyElementId>,
    ) -> Option<&mut Self> {
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

    pub(crate) fn element_index(&self, id: PropertyElementId) -> Option<usize> {
        let Self::Array(elements) = self else {
            return None;
        };
        elements
            .iter()
            .position(|element| element.element_id() == id)
    }

    pub(crate) fn f32_tuple<const N: usize>(values: [f32; N]) -> Self {
        Self::Tuple(values.into_iter().map(Self::F32).collect())
    }

    pub(in crate::domain) fn from_json(value: &Value, ty: &PropertyType) -> Option<Self> {
        match ty {
            PropertyType::Value(ty) => Self::value_type_from_json(value, ty),
            PropertyType::Array {
                element_type,
                min_items,
                max_items,
            } => {
                let values = value.as_array()?;
                if values.len() < *min_items as usize || values.len() > *max_items as usize {
                    return None;
                }
                values
                    .iter()
                    .enumerate()
                    .map(|(index, value)| {
                        Some(PropertyElement {
                            id: PropertyElementId(u64::try_from(index).ok()?.checked_add(1)?),
                            value: Self::value_type_from_json(value, element_type)?,
                        })
                    })
                    .collect::<Option<Vec<_>>>()
                    .map(Self::Array)
            }
        }
    }

    fn value_type_from_json(value: &Value, ty: &PropertyValueType) -> Option<Self> {
        match ty {
            PropertyValueType::Scalar(ty) => Self::scalar_from_json(value, ty),
            PropertyValueType::Tuple(tuple) => {
                let values = value.as_array()?;
                if values.len() != tuple.scalar_count() {
                    return None;
                }
                values
                    .iter()
                    .zip(tuple.scalars())
                    .map(|(value, ty)| Self::scalar_from_json(value, ty))
                    .collect::<Option<Vec<_>>>()
                    .map(Self::Tuple)
            }
        }
    }

    fn scalar_from_json(value: &Value, ty: &ScalarPropertyType) -> Option<Self> {
        match ty {
            ScalarPropertyType::Enum(ty) => {
                let value = u32::try_from(value.as_u64()?).ok()?;
                ty.contains(value).then_some(Self::Enum(value))
            }
            ScalarPropertyType::F32 => {
                let value = value.as_f64()?;
                let value = value as f32;
                value.is_finite().then_some(Self::F32(value))
            }
            ScalarPropertyType::I32 => i32::try_from(value.as_i64()?).ok().map(Self::I32),
            ScalarPropertyType::U32 => u32::try_from(value.as_u64()?).ok().map(Self::U32),
            ScalarPropertyType::Bool => value.as_bool().map(Self::Bool),
            ScalarPropertyType::Color => {
                let values = value.as_array()?;
                (values.len() == 4).then(|| {
                    Some(Self::Color([
                        json_f32(&values[0])?,
                        json_f32(&values[1])?,
                        json_f32(&values[2])?,
                        json_f32(&values[3])?,
                    ]))
                })?
            }
            ScalarPropertyType::String => value
                .as_str()
                .filter(|value| value.len() <= MAX_STRING_BYTES)
                .map(|value| Self::String(value.to_owned())),
        }
    }

    pub(crate) fn push_element(&mut self, value: PropertyValue) -> bool {
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

fn json_f32(value: &Value) -> Option<f32> {
    let value = value.as_f64()? as f32;
    value.is_finite().then_some(value)
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct PropertyValues {
    owner: Option<(String, String)>,
    schemas: HashMap<String, PropertySchema>,
    values: HashMap<String, PropertyValue>,
}

impl PropertyValues {
    pub(crate) fn empty() -> Self {
        Self {
            owner: None,
            schemas: HashMap::new(),
            values: HashMap::new(),
        }
    }

    pub(crate) fn from_properties(properties: &[PropertySchema]) -> Self {
        Self::from_owner(None, properties)
    }

    pub(in crate::domain) fn for_owner(
        owner_kind: &str,
        owner_id: &str,
        properties: &[PropertySchema],
    ) -> Self {
        Self::from_owner(
            Some((owner_kind.to_owned(), owner_id.to_owned())),
            properties,
        )
    }

    fn from_owner(owner: Option<(String, String)>, properties: &[PropertySchema]) -> Self {
        Self {
            owner,
            schemas: properties
                .iter()
                .map(|property| (property.id.clone(), property.clone()))
                .collect(),
            values: properties
                .iter()
                .map(|property| (property.id.clone(), property.default_value().clone()))
                .collect(),
        }
    }

    pub(crate) fn property(&self, id: &str) -> Option<&PropertyValue> {
        self.values.get(id)
    }

    pub(crate) fn property_mut(&mut self, id: &str) -> Option<&mut PropertyValue> {
        self.values.get_mut(id)
    }

    pub(crate) fn remove(&mut self, id: &str) -> Option<PropertyValue> {
        if self.owner.is_some() {
            return None;
        }
        self.schemas.remove(id);
        self.values.remove(id)
    }

    pub(crate) fn iter(&self) -> impl Iterator<Item = (&str, &PropertyValue)> {
        self.values.iter().map(|(id, value)| (id.as_str(), value))
    }

    pub(crate) fn set(
        &mut self,
        property: &PropertySchema,
        value: PropertyValue,
    ) -> Result<bool, PropertyError> {
        let existing_schema = self.schemas.get(&property.id);
        if self.owner.is_some() && existing_schema.is_none_or(|schema| schema != property) {
            return Err(PropertyError::invalid_definition(format!(
                "property '{}' belongs to a different schema contract",
                property.id
            )));
        }
        if !property.accepts_value(&value) {
            return Err(PropertyError::invalid_definition(format!(
                "property '{}' value violates its schema contract",
                property.id
            )));
        }
        if self.property(&property.id) == Some(&value) {
            return Ok(false);
        }
        self.schemas.insert(property.id.clone(), property.clone());
        self.values.insert(property.id.clone(), value);
        Ok(true)
    }

    pub(in crate::domain) fn validate_for(
        &self,
        owner_kind: &str,
        owner_id: &str,
        properties: &[PropertySchema],
    ) -> Result<(), PropertyError> {
        if self
            .owner
            .as_ref()
            .is_some_and(|(kind, id)| kind.as_str() != owner_kind || id.as_str() != owner_id)
        {
            return Err(PropertyError::invalid_definition(format!(
                "property values belong to a different owner than {owner_kind} '{owner_id}'"
            )));
        }
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
            if self.schemas.get(property.id()) != Some(property) || !property.accepts_value(value) {
                return Err(PropertyError::invalid_definition(format!(
                    "{owner_kind} '{owner_id}' property '{}' does not match its schema contract",
                    property.id()
                )));
            }
        }
        Ok(())
    }
}

pub(crate) fn materialized_property_values(
    overrides: &PropertyValues,
    schema: &[PropertySchema],
) -> PropertyValues {
    let mut values = PropertyValues::from_properties(schema);
    for property in schema {
        let Some(value) = overrides
            .property(&property.id)
            .and_then(|value| property.constrained_value(value))
        else {
            continue;
        };
        values
            .set(property, value)
            .expect("constrained values preserve the property type");
    }
    values
}
