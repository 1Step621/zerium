//! Errors from decoding and reconstructing persisted domain state.
use thiserror::Error;

#[derive(Debug, Error)]
pub(crate) enum ProjectError {
    #[error("{message}")]
    Encode {
        message: String,
        #[source]
        source: serde_json::Error,
    },
    #[error("{message}")]
    InvalidFormat {
        message: String,
        #[source]
        source: serde_json::Error,
    },
    #[error("{0}")]
    InvalidData(String),
    #[error("{0}")]
    UnsupportedFormat(String),
    #[error("{message}")]
    Io {
        message: String,
        #[source]
        source: std::io::Error,
    },
}

impl ProjectError {
    pub(crate) fn invalid_data(message: impl Into<String>) -> Self {
        Self::InvalidData(message.into())
    }

    pub(super) fn encode(message: impl Into<String>, source: serde_json::Error) -> Self {
        Self::Encode {
            message: message.into(),
            source,
        }
    }

    pub(super) fn invalid_format(message: impl Into<String>, source: serde_json::Error) -> Self {
        Self::InvalidFormat {
            message: message.into(),
            source,
        }
    }

    pub(super) fn unsupported_format(message: impl Into<String>) -> Self {
        Self::UnsupportedFormat(message.into())
    }

    pub(crate) fn io(message: impl Into<String>, source: std::io::Error) -> Self {
        Self::Io {
            message: message.into(),
            source,
        }
    }
}
