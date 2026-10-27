//! Filesystem boundary for project and scene documents.

use std::{
    fs,
    path::{Path, PathBuf},
};

use crate::app::storage::AtomicFileTransaction;
use zerium_core::{
    persistence::{self, LoadedDocument, ProjectError},
    plugin::PluginRegistry,
    timeline::{SceneId, TimelineSnapshot},
};

pub(crate) fn save(snapshot: &TimelineSnapshot, path: &Path) -> Result<(), ProjectError> {
    write(path, &persistence::encode(snapshot, path)?)
}

pub(crate) fn save_scene(
    snapshot: &TimelineSnapshot,
    root: SceneId,
    path: &Path,
) -> Result<(), ProjectError> {
    write(path, &persistence::encode_scene(snapshot, root, path)?)
}

fn write(path: &Path, encoded: &str) -> Result<(), ProjectError> {
    let transaction = AtomicFileTransaction::new(path).map_err(|error| {
        ProjectError::io(
            format!(
                "Failed to create temporary file for '{}': {error}",
                path.display()
            ),
            error,
        )
    })?;
    transaction.write_all(encoded.as_bytes()).map_err(|error| {
        ProjectError::io(
            format!(
                "Failed to write temporary file for '{}': {error}",
                path.display()
            ),
            error,
        )
    })?;
    transaction.commit().map_err(|error| {
        ProjectError::io(
            format!("Failed to finalize '{}': {error}", path.display()),
            error,
        )
    })
}

pub(crate) fn load(path: &Path, plugins: &PluginRegistry) -> Result<LoadedDocument, ProjectError> {
    persistence::decode_document(&read(path)?, path, plugins)
}

/// Validate scene contents and return the canonical path for registration.
pub(crate) fn validate_scene_file(
    path: &Path,
    plugins: &PluginRegistry,
) -> Result<PathBuf, ProjectError> {
    let path = fs::canonicalize(path).map_err(|error| {
        ProjectError::io(
            format!("Failed to resolve '{}': {error}", path.display()),
            error,
        )
    })?;
    if let LoadedDocument::Scene(_) = load(&path, plugins)? {
        Ok(path)
    } else {
        Err(ProjectError::invalid_data(
            "The selected file is not a scene file",
        ))
    }
}

fn read(path: &Path) -> Result<String, ProjectError> {
    fs::read_to_string(path).map_err(|error| {
        ProjectError::io(
            format!("Failed to open '{}': {error}", path.display()),
            error,
        )
    })
}
