use std::{error::Error, fmt};

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
