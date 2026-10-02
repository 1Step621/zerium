use thiserror::Error;

/// Failure to decode, validate, or resolve a plugin bundle.
#[derive(Debug, Error)]
pub enum PluginError {
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
    pub fn invalid_definition(message: impl Into<String>) -> Self {
        Self::InvalidDefinition(message.into())
    }

    pub fn invalid_manifest(message: impl Into<String>, source: serde_json::Error) -> Self {
        Self::InvalidManifest {
            message: message.into(),
            source,
        }
    }

    pub fn missing_asset(message: impl Into<String>) -> Self {
        Self::MissingAsset(message.into())
    }
}

impl From<crate::property::PropertyError> for PluginError {
    fn from(error: crate::property::PropertyError) -> Self {
        Self::invalid_definition(error.to_string())
    }
}
