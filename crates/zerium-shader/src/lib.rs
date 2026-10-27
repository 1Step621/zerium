//! Shader tools shared by the application and plugin CLI, without GPU or UI dependencies.

#![deny(unreachable_pub)]

use self::compile::source::{read_manifest, require_shader_modules};
use generate::contract::shader_contracts;
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};
use thiserror::Error;
use wesl::resolver::FileResolver;
use zerium_core::plugin::{PluginError, PluginRegistry};

mod compile;
mod generate;
mod validate;

pub use compile::source::{
    PluginBundle, PluginLibrary, load_embedded_plugin, load_filesystem_plugin,
};
pub use generate::{IMAGE_SAMPLER_BINDING, ImageBinding};
pub use validate::{
    WgslModule,
    descriptor::{
        ComputeShaderDescriptor, EffectShaderDescriptor, EffectShaderId, ItemShaderDescriptor,
        ItemShaderId, ValidatedEffectShader, ValidatedPluginShaders,
    },
    validate_plugins,
};
pub use wesl::syntax::ModulePath;
pub use zerium_core::plugin::MAX_TEXTURE_INPUTS;

/// Generate host and property interfaces from the plugin manifest.
pub fn generate(path: Option<&Path>) -> Result<PathBuf, ShaderError> {
    let root = plugin_directory(path)?;
    let manifest = read_manifest(&root)?;
    require_shader_modules(&manifest, &FileResolver::new(&root))?;
    let contracts = shader_contracts(&manifest)?;
    let modules = generate::generated_modules(&contracts);
    generate::install(&root, &modules)
}

/// Validate a plugin's shaders and return its editing catalog for host-specific checks.
pub fn validate(path: Option<&Path>) -> Result<Arc<PluginRegistry>, ShaderError> {
    let root = plugin_directory(path)?;
    let plugins = PluginLibrary::new([load_filesystem_plugin(&root)?])?;
    validate_plugins(&plugins)?;
    Ok(plugins.registry())
}

fn plugin_directory(path: Option<&Path>) -> Result<PathBuf, ShaderError> {
    let root = path
        .map(Path::to_owned)
        .map_or_else(std::env::current_dir, Ok)
        .map_err(|error| ShaderError::operation(error.to_string()))?;
    root.canonicalize()
        .map_err(|error| ShaderError::io("access plugin directory", &root, error))
}

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
