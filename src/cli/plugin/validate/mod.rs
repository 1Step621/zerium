//! Validation of a plugin directory before it is loaded by the application.

use std::path::Path;

/// Validate a plugin directory using the same WESL and WGSL checks as runtime
/// pipeline registration.
pub(crate) fn validate(path: Option<&Path>) -> Result<(), String> {
    let root = path
        .map(Path::to_owned)
        .unwrap_or(std::env::current_dir().map_err(|error| error.to_string())?);
    let root = root.canonicalize().map_err(|error| {
        format!(
            "cannot access plugin directory '{}': {error}",
            root.display()
        )
    })?;
    let plugin = crate::plugin::load_filesystem_plugin(&root).map_err(|error| error.to_string())?;
    crate::engine::rendering::validate_plugin(&plugin).map_err(|error| error.to_string())
}
