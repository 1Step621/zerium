//! Filesystem entry points used by the plugin CLI.

use std::path::{Path, PathBuf};

use wesl::resolver::FileResolver;

use crate::{
    PluginLibrary, ShaderError, compile_plugins,
    generate::{self, contract::shader_contracts},
    loader::{load_filesystem_plugin, read_manifest},
    source::require_shader_modules,
};

/// Generate host and property interfaces without compiling the plugin shaders.
pub fn generate(path: Option<&Path>) -> Result<PathBuf, ShaderError> {
    let root = plugin_directory(path)?;
    let manifest = read_manifest(&root)?;
    require_shader_modules(&manifest, &FileResolver::new(&root))?;
    let contracts = shader_contracts(&manifest)?;
    let files = generate::generated_files(&contracts);
    generate::output::install(&root, &files)
}

/// Validate a plugin with the same linking and WGSL checks used by the application.
pub fn validate(path: Option<&Path>) -> Result<(), ShaderError> {
    let root = plugin_directory(path)?;
    let plugins = PluginLibrary::new([load_filesystem_plugin(&root)?])?;
    compile_plugins(&plugins).map(|_| ())
}

fn plugin_directory(path: Option<&Path>) -> Result<PathBuf, ShaderError> {
    let root = path
        .map(Path::to_owned)
        .map_or_else(std::env::current_dir, Ok)
        .map_err(|error| ShaderError::operation(error.to_string()))?;
    root.canonicalize()
        .map_err(|error| ShaderError::io("access plugin directory", &root, error))
}
