use std::{error::Error, fmt};

/// Failure to decode, validate, or resolve a plugin bundle.
#[derive(Debug)]
pub(crate) enum PluginError {
    InvalidManifest {
        message: String,
        source: serde_json::Error,
    },
    InvalidDefinition(String),
    MissingAsset(String),
}

impl PluginError {
    pub(crate) fn invalid_definition(message: impl Into<String>) -> Self {
        Self::InvalidDefinition(message.into())
    }

    pub(crate) fn invalid_manifest(message: impl Into<String>, source: serde_json::Error) -> Self {
        Self::InvalidManifest {
            message: message.into(),
            source,
        }
    }

    pub(crate) fn missing_asset(message: impl Into<String>) -> Self {
        Self::MissingAsset(message.into())
    }
}

impl fmt::Display for PluginError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::InvalidManifest { message, .. }
            | Self::InvalidDefinition(message)
            | Self::MissingAsset(message) => message,
        })
    }
}

impl Error for PluginError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidManifest { source, .. } => Some(source),
            Self::InvalidDefinition(_) | Self::MissingAsset(_) => None,
        }
    }
}

impl From<crate::domain::parameter::ParameterError> for PluginError {
    fn from(error: crate::domain::parameter::ParameterError) -> Self {
        Self::invalid_definition(error.to_string())
    }
}
