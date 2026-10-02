//! Shader tools shared by the application and plugin CLI, without GPU or UI dependencies.

#![deny(unreachable_pub)]

pub mod capability_input;
mod compile;
mod contract;
mod generate;
mod loader;
mod types;
mod wesl;

use std::path::Path;

use thiserror::Error;
use zerium_core::plugin::{PluginError, PluginRegistry};

pub use compile::{compile_plugins, validate_compute_shader, validate_render_shader};
pub use contract::{ShaderContract, ShaderProperty, shader_contract_fingerprint, shader_contracts};
pub use generate::generate;
pub use loader::{load_filesystem_plugin, plugin_from_parts};
pub use types::{
    CompiledEffectShader, CompiledPluginShaders, ComputeShaderDescriptor, EffectShaderDescriptor,
    EffectShaderId, ItemShaderDescriptor, ItemShaderId, TextureShaderDescriptor,
};

#[derive(Debug, Error)]
pub enum ShaderError {
    #[error("{0}")]
    Operation(String),
    #[error(transparent)]
    Plugin(#[from] PluginError),
}

impl ShaderError {
    pub(crate) fn backend(message: impl Into<String>) -> Self {
        Self::Operation(message.into())
    }
}

/// Validate a plugin with the same linking and WGSL checks used by the application.
pub fn validate(path: Option<&Path>) -> Result<(), ShaderError> {
    let root = path
        .map(Path::to_owned)
        .map_or_else(std::env::current_dir, Ok)
        .map_err(|error| ShaderError::backend(error.to_string()))?;
    let root = root.canonicalize().map_err(|error| {
        ShaderError::backend(format!(
            "cannot access plugin directory '{}': {error}",
            root.display()
        ))
    })?;
    let plugins = PluginRegistry::new([load_filesystem_plugin(&root)?])?;
    compile_plugins(&plugins).map(|_| ())
}
