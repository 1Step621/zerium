//! Runtime parameter values and checked value collections.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{
    ParameterError,
    schema::ParameterSchema,
    types::{ParameterType, ParameterValueType, ScalarParameterType},
};
use crate::domain::animation::ParameterAnimationAddress;

pub(in crate::domain) const MAX_STRING_BYTES: usize = 4_096;

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub(crate) struct ArrayElementId(u64);

impl ArrayElementId {
    pub(crate) const fn is_valid(self) -> bool {
        self.0 != 0
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ArrayElement {
    id: ArrayElementId,
    value: ParameterValue,
}

impl ArrayElement {
    pub(crate) const fn id(&self) -> ArrayElementId {
        self.id
    }

    pub(crate) const fn value(&self) -> &ParameterValue {
        &self.value
    }

    pub(crate) const fn value_mut(&mut self) -> &mut ParameterValue {
        &mut self.value
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
pub(crate) enum ParameterValue {
    F32(f32),
    I32(i32),
    U32(u32),
    Enum(u32),
    Bool(bool),
    Tuple(Vec<ParameterValue>),
    Color([f32; 4]),
    String(String),
    Array(Vec<ArrayElement>),
}

impl ParameterValue {
    pub(crate) fn scalar_at(&self, element: Option<usize>) -> Option<&Self> {
        match (self, element) {
            (Self::Tuple(values), Some(index)) => values.get(index),
            (Self::Tuple(_) | Self::Array(_), _) => None,
            (value, None) => Some(value),
            _ => None,
        }
    }

    pub(crate) fn scalar_at_mut(&mut self, element: Option<usize>) -> Option<&mut Self> {
        match (self, element) {
            (Self::Tuple(values), Some(index)) => values.get_mut(index),
            (Self::Tuple(_) | Self::Array(_), _) => None,
            (value, None) => Some(value),
            _ => None,
        }
    }

    pub(crate) fn f32_tuple<const N: usize>(values: [f32; N]) -> Self {
        Self::Tuple(values.into_iter().map(Self::F32).collect())
    }

    pub(crate) fn to_json_value(&self) -> Value {
        match self {
            Self::F32(value) => serde_json::json!(value),
            Self::I32(value) => serde_json::json!(value),
            Self::U32(value) | Self::Enum(value) => serde_json::json!(value),
            Self::Bool(value) => serde_json::json!(value),
            Self::Tuple(values) => Value::Array(values.iter().map(Self::to_json_value).collect()),
            Self::Color(value) => serde_json::json!(value),
            Self::String(value) => serde_json::json!(value),
            Self::Array(values) => Value::Array(
                values
                    .iter()
                    .map(|element| element.value.to_json_value())
                    .collect(),
            ),
        }
    }

    pub(in crate::domain) fn from_json(value: &Value, ty: &ParameterType) -> Option<Self> {
        match ty {
            ParameterType::Value(ty) => Self::value_type_from_json(value, ty),
            ParameterType::Array {
                element,
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
                        Some(ArrayElement {
                            id: ArrayElementId(u64::try_from(index).ok()?.checked_add(1)?),
                            value: Self::value_type_from_json(value, element)?,
                        })
                    })
                    .collect::<Option<Vec<_>>>()
                    .map(Self::Array)
            }
        }
    }

    fn value_type_from_json(value: &Value, ty: &ParameterValueType) -> Option<Self> {
        match ty {
            ParameterValueType::Scalar(ty) => Self::scalar_from_json(value, ty),
            ParameterValueType::Tuple(tuple) => {
                let values = value.as_array()?;
                if values.len() != tuple.element_count() {
                    return None;
                }
                values
                    .iter()
                    .zip(tuple.elements())
                    .map(|(value, ty)| Self::scalar_from_json(value, ty))
                    .collect::<Option<Vec<_>>>()
                    .map(Self::Tuple)
            }
        }
    }

    fn scalar_from_json(value: &Value, ty: &ScalarParameterType) -> Option<Self> {
        match ty {
            ScalarParameterType::Enum(ty) => {
                let value = u32::try_from(value.as_u64()?).ok()?;
                ty.contains(value).then_some(Self::Enum(value))
            }
            ScalarParameterType::F32 => {
                let value = value.as_f64()?;
                let value = value as f32;
                value.is_finite().then_some(Self::F32(value))
            }
            ScalarParameterType::I32 => i32::try_from(value.as_i64()?).ok().map(Self::I32),
            ScalarParameterType::U32 => u32::try_from(value.as_u64()?).ok().map(Self::U32),
            ScalarParameterType::Bool => value.as_bool().map(Self::Bool),
            ScalarParameterType::Color => {
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
            ScalarParameterType::String => value
                .as_str()
                .filter(|value| value.len() <= MAX_STRING_BYTES)
                .map(|value| Self::String(value.to_owned())),
        }
    }

    pub(crate) fn push_array_element(&mut self, value: ParameterValue) -> bool {
        let Self::Array(elements) = self else {
            return false;
        };
        let Some(id) = elements
            .iter()
            .map(|element| element.id.0)
            .max()
            .unwrap_or(0)
            .checked_add(1)
        else {
            return false;
        };
        elements.push(ArrayElement {
            id: ArrayElementId(id),
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
struct ParameterContract {
    ty: ParameterType,
    constraints: super::ParameterConstraints,
}

impl ParameterContract {
    fn from_schema(parameter: &ParameterSchema) -> Self {
        Self {
            ty: parameter.ty.clone(),
            constraints: parameter.constraints.clone(),
        }
    }

    fn accepts(&self, value: &ParameterValue) -> bool {
        self.ty.allows(value) && self.constraints.allows(value)
    }
}

#[derive(Clone, Debug, PartialEq)]
struct StoredParameterValue {
    value: ParameterValue,
    contract: ParameterContract,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct ParameterValues {
    owner: Option<(String, String)>,
    values: HashMap<String, StoredParameterValue>,
}

impl ParameterValues {
    pub(crate) fn from_parameters(parameters: &[ParameterSchema]) -> Self {
        Self::from_owner(None, parameters)
    }

    pub(in crate::domain) fn for_owner(
        owner_kind: &str,
        owner_id: &str,
        parameters: &[ParameterSchema],
    ) -> Self {
        Self::from_owner(
            Some((owner_kind.to_owned(), owner_id.to_owned())),
            parameters,
        )
    }

    fn from_owner(owner: Option<(String, String)>, parameters: &[ParameterSchema]) -> Self {
        Self {
            owner,
            values: parameters
                .iter()
                .map(|parameter| {
                    (
                        parameter.id.clone(),
                        StoredParameterValue {
                            value: parameter.default_value().clone(),
                            contract: ParameterContract::from_schema(parameter),
                        },
                    )
                })
                .collect(),
        }
    }

    pub(crate) fn get(&self, id: &str) -> Option<&ParameterValue> {
        self.values.get(id).map(|stored| &stored.value)
    }

    pub(crate) fn get_at(&self, address: &ParameterAnimationAddress) -> Option<&ParameterValue> {
        let value = self.get(&address.parameter_id)?;
        match address.array_element_id {
            Some(id) => match value {
                ParameterValue::Array(values) => values
                    .iter()
                    .find(|element| element.id == id)
                    .map(ArrayElement::value),
                _ => None,
            },
            None => Some(value),
        }
    }

    pub(crate) fn get_at_mut(
        &mut self,
        address: &ParameterAnimationAddress,
    ) -> Option<&mut ParameterValue> {
        let value = &mut self.values.get_mut(&address.parameter_id)?.value;
        match address.array_element_id {
            Some(id) => match value {
                ParameterValue::Array(values) => values
                    .iter_mut()
                    .find(|element| element.id == id)
                    .map(ArrayElement::value_mut),
                _ => None,
            },
            None => Some(value),
        }
    }

    pub(crate) fn get_scalar_at(
        &self,
        address: &ParameterAnimationAddress,
    ) -> Option<&ParameterValue> {
        self.get_at(address)?
            .scalar_at(address.channel.coordinate())
    }

    pub(crate) fn get_scalar_at_mut(
        &mut self,
        address: &ParameterAnimationAddress,
    ) -> Option<&mut ParameterValue> {
        self.get_at_mut(address)?
            .scalar_at_mut(address.channel.coordinate())
    }

    pub(crate) fn scalar_at<'a>(
        &self,
        address: &ParameterAnimationAddress,
        ty: &'a ParameterType,
    ) -> Option<(&ParameterValue, &'a ScalarParameterType)> {
        let value = self.get_scalar_at(address)?;
        let value_type = match address.array_element_id {
            Some(_) => ty.array_element_type()?,
            None => ty.value_type()?,
        };
        let scalar_type = value_type.scalar_at(address.channel.coordinate())?;
        Some((value, scalar_type))
    }

    pub(crate) fn array_element_id(
        &self,
        parameter_id: &str,
        index: usize,
    ) -> Option<ArrayElementId> {
        let ParameterValue::Array(elements) = self.get(parameter_id)? else {
            return None;
        };
        elements.get(index).map(ArrayElement::id)
    }

    pub(crate) fn array_element_index(
        &self,
        parameter_id: &str,
        id: ArrayElementId,
    ) -> Option<usize> {
        let ParameterValue::Array(elements) = self.get(parameter_id)? else {
            return None;
        };
        elements.iter().position(|element| element.id == id)
    }

    pub(crate) fn remove(&mut self, id: &str) -> Option<ParameterValue> {
        if self.owner.is_some() {
            return None;
        }
        self.values.remove(id).map(|stored| stored.value)
    }

    pub(crate) fn iter(&self) -> impl Iterator<Item = (&str, &ParameterValue)> {
        self.values
            .iter()
            .map(|(id, stored)| (id.as_str(), &stored.value))
    }

    pub(crate) fn set(
        &mut self,
        parameter: &ParameterSchema,
        value: ParameterValue,
    ) -> Result<bool, ParameterError> {
        let supplied_contract = ParameterContract::from_schema(parameter);
        let existing_contract = self
            .values
            .get(&parameter.id)
            .map(|stored| &stored.contract);
        if self.owner.is_some()
            && existing_contract.is_none_or(|contract| contract != &supplied_contract)
        {
            return Err(ParameterError::invalid_definition(format!(
                "parameter '{}' belongs to a different schema contract",
                parameter.id
            )));
        }
        if !supplied_contract.accepts(&value) {
            return Err(ParameterError::invalid_definition(format!(
                "parameter '{}' value violates its schema contract",
                parameter.id
            )));
        }
        if self.get(&parameter.id) == Some(&value) {
            return Ok(false);
        }
        self.values.insert(
            parameter.id.clone(),
            StoredParameterValue {
                value,
                contract: supplied_contract,
            },
        );
        Ok(true)
    }

    pub(in crate::domain) fn validate_for(
        &self,
        owner_kind: &str,
        owner_id: &str,
        parameters: &[ParameterSchema],
    ) -> Result<(), ParameterError> {
        if self
            .owner
            .as_ref()
            .is_some_and(|(kind, id)| kind.as_str() != owner_kind || id.as_str() != owner_id)
        {
            return Err(ParameterError::invalid_definition(format!(
                "parameter values belong to a different owner than {owner_kind} '{owner_id}'"
            )));
        }
        if self.values.len() != parameters.len() {
            return Err(ParameterError::invalid_definition(format!(
                "{owner_kind} '{owner_id}' parameter set is incomplete"
            )));
        }
        for parameter in parameters {
            let stored = self.values.get(parameter.id()).ok_or_else(|| {
                ParameterError::invalid_definition(format!(
                    "{owner_kind} '{owner_id}' is missing parameter '{}'",
                    parameter.id()
                ))
            })?;
            if stored.contract != ParameterContract::from_schema(parameter)
                || !stored.contract.accepts(&stored.value)
            {
                return Err(ParameterError::invalid_definition(format!(
                    "{owner_kind} '{owner_id}' parameter '{}' does not match its schema contract",
                    parameter.id()
                )));
            }
        }
        Ok(())
    }
}

pub(crate) fn materialized_parameter_values(
    overrides: &ParameterValues,
    schema: &[ParameterSchema],
) -> ParameterValues {
    let mut values = ParameterValues::from_parameters(schema);
    for parameter in schema {
        let Some(value) = overrides
            .get(&parameter.id)
            .and_then(|value| parameter.constrained_value(value))
        else {
            continue;
        };
        values
            .set(parameter, value)
            .expect("constrained values preserve the parameter type");
    }
    values
}
