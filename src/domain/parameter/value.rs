//! Runtime parameter values and checked value collections.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{
    ParameterError,
    schema::ParameterSchema,
    types::{ParameterType, ParameterValueType, ScalarParameterType},
};

pub(in crate::domain) const MAX_STRING_BYTES: usize = 4_096;

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
    Array(Vec<ParameterValue>),
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

    pub(crate) fn animated_array_element(&self, index: usize) -> Option<Self> {
        let Self::Array(values) = self else {
            return None;
        };
        Some(values.get(index)?.clone())
    }

    pub(crate) fn with_animated_array_element(&self, index: usize, value: Self) -> Option<Self> {
        let Self::Array(values) = self else {
            return None;
        };
        let mut values = values.clone();
        *values.get_mut(index)? = value;
        Some(Self::Array(values))
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
            Self::Array(values) => Value::Array(values.iter().map(Self::to_json_value).collect()),
        }
    }

    pub(crate) fn matches_type(&self, ty: &ParameterType) -> bool {
        match ty {
            ParameterType::Array {
                element,
                min_items,
                max_items,
            } => {
                let Self::Array(values) = self else {
                    return false;
                };
                values.len() >= *min_items as usize
                    && values.len() <= *max_items as usize
                    && values.iter().all(|value| value.matches_value_type(element))
            }
            ParameterType::Value(ty) => self.matches_value_type(ty),
        }
    }

    fn matches_value_type(&self, ty: &ParameterValueType) -> bool {
        match ty {
            ParameterValueType::Scalar(ty) => self.matches_scalar(ty),
            ParameterValueType::Tuple(tuple) => {
                let Self::Tuple(values) = self else {
                    return false;
                };
                values.len() == tuple.element_count()
                    && values
                        .iter()
                        .zip(tuple.elements())
                        .all(|(value, ty)| value.matches_scalar(ty))
            }
        }
    }

    pub(crate) fn matches_scalar(&self, ty: &ScalarParameterType) -> bool {
        match (self, ty) {
            (Self::F32(value), ScalarParameterType::F32) => value.is_finite(),
            (Self::I32(_), ScalarParameterType::I32)
            | (Self::U32(_), ScalarParameterType::U32)
            | (Self::Bool(_), ScalarParameterType::Bool) => true,
            (Self::String(value), ScalarParameterType::String) => value.len() <= MAX_STRING_BYTES,
            (Self::Color(values), ScalarParameterType::Color) => {
                values.iter().all(|value| value.is_finite())
            }
            (Self::Enum(value), ScalarParameterType::Enum(ty)) => ty.contains(*value),
            _ => false,
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
                    .map(|value| Self::value_type_from_json(value, element))
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
        value.matches_type(&self.ty) && self.constraints.allows(value)
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
