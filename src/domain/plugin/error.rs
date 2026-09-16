use thiserror::Error;

/// Failure to decode, validate, or resolve a plugin bundle.
#[derive(Debug, Error)]
pub(crate) enum PluginError {
    #[error("{message}")]
    InvalidManifest {
        message: String,
        #[source]
        source: serde_json::Error,
    },
    #[error("{0}")]
    InvalidDefinition(String),
    #[error("{0}")]
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

impl From<crate::domain::property::PropertyError> for PluginError {
    fn from(error: crate::domain::property::PropertyError) -> Self {
        Self::invalid_definition(error.to_string())
    }
}
