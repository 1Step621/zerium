//! Shared parameter contracts, values, validation, and editor metadata.
use std::{error::Error, fmt};

mod constraints;
mod numeric;
mod schema;
mod types;
mod ui;
mod value;
mod wire;

pub(crate) use constraints::ParameterConstraints;
pub(crate) use numeric::NumericSettings;
pub(crate) use schema::{ParameterAnimatable, ParameterEditable, ParameterSchema};
pub(crate) use types::{ParameterType, ParameterValueType, ScalarParameterType};
pub(crate) use ui::ParameterUi;
pub(in crate::domain) use value::MAX_STRING_BYTES;
pub(crate) use value::materialized_parameter_values;
pub(crate) use value::{
    ArrayElement, ArrayElementId, ParameterAddress, ParameterValue, ParameterValuePath,
    ParameterValues,
};

/// A value or schema violates its parameter contract.
#[derive(Debug)]
pub(crate) struct ParameterError(String);

impl ParameterError {
    pub(crate) fn invalid_definition(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

impl fmt::Display for ParameterError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl Error for ParameterError {}
