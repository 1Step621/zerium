//! Errors from decoding and reconstructing persisted domain state.
use std::{error::Error, fmt};

#[derive(Debug)]
pub(crate) enum ProjectError {
    Encode {
        message: String,
        source: serde_json::Error,
    },
    InvalidFormat {
        message: String,
        source: serde_json::Error,
    },
    InvalidData(String),
    UnsupportedFormat(String),
    Io {
        message: String,
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

impl fmt::Display for ProjectError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Encode { message, .. }
            | Self::InvalidFormat { message, .. }
            | Self::Io { message, .. }
            | Self::InvalidData(message)
            | Self::UnsupportedFormat(message) => message,
        })
    }
}

impl Error for ProjectError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Encode { source, .. } | Self::InvalidFormat { source, .. } => Some(source),
            Self::Io { source, .. } => Some(source),
            Self::InvalidData(_) | Self::UnsupportedFormat(_) => None,
        }
    }
}
