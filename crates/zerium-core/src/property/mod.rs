//! Shared property contracts, values, validation, and editor metadata.
use thiserror::Error;

mod constraints;
mod declaration;
mod metadata;
mod numeric;
mod path;
mod schema;
mod types;
mod value;

pub use constraints::PropertyConstraints;
pub use metadata::PropertyUi;
pub use numeric::NumericSettings;
pub use path::PropertyPath;
pub use schema::{
    PropertyConfiguration, PropertyDefinition, PropertySchema, ScalarSchema, ValueSchema,
};
pub use types::{EnumPropertyType, EnumVariant, ScalarPropertyType};
pub(crate) use value::MAX_STRING_BYTES;
pub use value::materialized_property_values;
pub use value::{PropertyElement, PropertyElementId, PropertyValue, PropertyValues};

/// A value or schema violates its property contract.
#[derive(Debug, Error)]
#[error("{0}")]
pub struct PropertyError(String);

impl PropertyError {
    pub fn invalid_definition(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}
