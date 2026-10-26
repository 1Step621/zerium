//! Persisted representations and checked reconstruction; filesystem I/O lives in app::project::file.
mod clipboard;
mod error;
mod items;
mod project;
mod scene;

pub use clipboard::{
    DecodedTimelineClipboard, decode_timeline_clipboard, encode_timeline_clipboard,
};
pub use error::ProjectError;
pub use project::{LoadedProject, PROJECT_EXTENSION, decode, encode};
pub use scene::{LoadedScene, SCENE_FILE_SUFFIX, decode_scene, encode_scene};

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::plugin::PluginRegistry;

pub enum LoadedDocument {
    Project(LoadedProject),
    Scene(LoadedScene),
}

#[derive(Clone, Copy, Default, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum DocumentKind {
    #[default]
    Project,
    Scene,
}

/// The filename is only a convention; the file contents determine how it opens.
pub fn decode_document(
    source: &str,
    path: &Path,
    plugins: &PluginRegistry,
) -> Result<LoadedDocument, ProjectError> {
    #[derive(Deserialize)]
    struct Header {
        // Existing project formats have no document type marker.
        #[serde(default, rename = "type")]
        kind: DocumentKind,
    }
    let header: Header = serde_json::from_str(source).map_err(|error| {
        ProjectError::invalid_format(format!("Invalid Zerium file: {error}"), error)
    })?;
    match header.kind {
        DocumentKind::Project => decode(source, path, plugins).map(LoadedDocument::Project),
        DocumentKind::Scene => decode_scene(source, path, plugins).map(LoadedDocument::Scene),
    }
}

fn make_relative(path: &Path, project_path: &Path) -> PathBuf {
    let Some(directory) = project_path.parent() else {
        return path.to_path_buf();
    };
    path.strip_prefix(directory)
        .map(Path::to_path_buf)
        .unwrap_or_else(|_| path.to_path_buf())
}

fn resolve_path(path: &Path, project_path: &Path) -> PathBuf {
    if path.is_absolute() {
        return path.to_path_buf();
    }
    project_path
        .parent()
        .map(|directory| directory.join(path))
        .unwrap_or_else(|| path.to_path_buf())
}
