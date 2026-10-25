//! Shared property contracts, values, validation, and editor metadata.
use thiserror::Error;

mod path;
mod schema;
mod value;

pub use path::PropertyPath;
pub use schema::{
    ArraySchema, PropertyConfiguration, PropertyDefinition, PropertySchema, ScalarSchema,
    ValueSchema,
    constraints::{NumericSettings, PropertyConstraints},
    metadata::PropertyUi,
    types::{EnumPropertyType, EnumVariant, ScalarPropertyType},
};
pub(crate) use value::MAX_STRING_BYTES;
pub use value::{
    PropertyElement, PropertyElementId, PropertyOverrides, PropertyValue, PropertyValues,
};

/// A value or schema violates its property contract.
#[derive(Debug, Error)]
#[error("{0}")]
pub struct PropertyError(String);

impl PropertyError {
    pub fn invalid_definition(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}
