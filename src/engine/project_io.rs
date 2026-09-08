//! Filesystem boundary for Zerium project documents.

use std::{fs, path::Path};

use crate::{
    domain::{
        persistence::{self, LoadedProject, ProjectError},
        plugin::PluginRegistry,
        timeline::TimelineSnapshot,
    },
    engine::media::AtomicFileTransaction,
};

pub(crate) fn save(snapshot: &TimelineSnapshot, path: &Path) -> Result<(), ProjectError> {
    let encoded = persistence::encode(snapshot, path)?;
    let transaction = AtomicFileTransaction::new(path).map_err(|error| {
        ProjectError::io(
            format!(
                "プロジェクト '{}' の一時ファイルを作成できません: {error}",
                path.display()
            ),
            error,
        )
    })?;
    transaction.write_all(encoded.as_bytes()).map_err(|error| {
        ProjectError::io(
            format!(
                "プロジェクト '{}' の一時ファイルを書き込めません: {error}",
                path.display()
            ),
            error,
        )
    })?;
    transaction.commit().map_err(|error| {
        ProjectError::io(
            format!(
                "プロジェクト '{}' を確定できません: {error}",
                path.display()
            ),
            error,
        )
    })
}

pub(crate) fn load(path: &Path, plugins: &PluginRegistry) -> Result<LoadedProject, ProjectError> {
    let source = fs::read_to_string(path).map_err(|error| {
        ProjectError::io(
            format!("プロジェクト '{}' を開けません: {error}", path.display()),
            error,
        )
    })?;
    persistence::decode(&source, path, plugins)
}
