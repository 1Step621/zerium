//! Errors shared by plugin generation, loading, and compilation.

use std::path::Path;

use thiserror::Error;
use zerium_core::plugin::PluginError;

#[derive(Debug, Error)]
pub enum ShaderError {
    #[error("{0}")]
    Operation(String),
    #[error(transparent)]
    Plugin(#[from] PluginError),
}

impl ShaderError {
    pub(crate) fn operation(message: impl Into<String>) -> Self {
        Self::Operation(message.into())
    }

    pub(crate) fn io(action: &str, path: &Path, error: std::io::Error) -> Self {
        Self::operation(format!("cannot {action} '{}': {error}", path.display()))
    }
}
