//! Shared property contracts, values, validation, and editor metadata.
use thiserror::Error;

mod constraints;
mod metadata;
mod numeric;
mod schema;
mod types;
mod value;

pub(crate) use constraints::PropertyConstraints;
pub(crate) use numeric::NumericSettings;
pub(crate) use schema::{PropertyConfiguration, PropertySchema};
pub(crate) use types::{PropertyType, PropertyValueType, ScalarPropertyType};
pub(in crate::domain) use value::MAX_STRING_BYTES;
pub(crate) use value::materialized_property_values;
pub(crate) use value::{PropertyElement, PropertyElementId, PropertyValue, PropertyValues};

/// A value or schema violates its property contract.
#[derive(Debug, Error)]
#[error("{0}")]
pub(crate) struct PropertyError(String);

impl PropertyError {
    pub(crate) fn invalid_definition(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}
