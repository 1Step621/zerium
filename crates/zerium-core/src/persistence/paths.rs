//! File references shared by project capture and timeline clipboard capture.
use crate::timeline::TimelineItem;
use std::path::{Path, PathBuf};

pub(super) fn item_files(item: &TimelineItem) -> impl Iterator<Item = &Path> {
    std::iter::once(&item.properties)
        .chain(item.effects.iter().map(|effect| &effect.properties))
        .flat_map(|properties| properties.files().map(|(_, path)| path))
}

pub(super) fn make_relative(path: &Path, project_path: &Path) -> PathBuf {
    let Some(directory) = project_path.parent() else {
        return path.to_path_buf();
    };
    path.strip_prefix(directory)
        .map(Path::to_path_buf)
        .unwrap_or_else(|_| path.to_path_buf())
}

pub(super) fn resolve_path(path: &Path, project_path: &Path) -> PathBuf {
    if path.is_absolute() {
        return path.to_path_buf();
    }
    project_path
        .parent()
        .map(|directory| directory.join(path))
        .unwrap_or_else(|| path.to_path_buf())
}
