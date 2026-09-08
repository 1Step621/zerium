//! Shared parameter contracts, values, validation, and editor metadata.
mod compatibility;
mod constraints;
mod error;
mod numeric;
mod schema;
mod settings;
mod types;
mod ui;
mod validation;
mod value;
mod wire;

pub(crate) use constraints::ParameterConstraints;
pub(crate) use error::ParameterError;
pub(crate) use schema::ParameterSchema;
pub(crate) use settings::NumericSettings;
pub(crate) use types::{ParameterType, ParameterValueType, ScalarParameterType};
pub(crate) use ui::ParameterUi;
pub(in crate::domain) use value::MAX_STRING_BYTES;
pub(crate) use value::materialized_parameter_values;
pub(crate) use value::{ParameterValue, ParameterValues};
