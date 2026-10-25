//! Persisted representations and checked reconstruction; filesystem I/O lives in app::project::file.
mod clipboard;
mod error;
mod items;
mod project;

pub use clipboard::{
    DecodedTimelineClipboard, decode_timeline_clipboard, encode_timeline_clipboard,
};
pub use error::ProjectError;
pub use project::{LoadedProject, PROJECT_EXTENSION, decode, encode};

use std::path::{Path, PathBuf};

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
