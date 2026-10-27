//! User-selected settings. Unspecified options follow the application's defaults.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

use super::JsonStore;
use crate::app::defaults::CreationDefaults;

pub(crate) type ConfigStore = JsonStore<AppConfig>;

#[derive(Clone, Default, Deserialize, Serialize)]
#[serde(default)]
pub(crate) struct AppConfig {
    #[serde(skip_serializing_if = "CreationDefaults::is_empty")]
    pub(crate) creation_defaults: CreationDefaults,
    #[serde(skip_serializing_if = "SceneLibrary::is_empty")]
    pub(crate) scene_library: SceneLibrary,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) language: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) show_support_reminder: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) theme_mode: Option<ui::ThemeMode>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) accent_color: Option<AccentColor>,
}

#[derive(Clone, Default, Deserialize, Serialize)]
#[serde(transparent)]
pub(crate) struct SceneLibrary {
    files: Vec<PathBuf>,
}

impl SceneLibrary {
    pub(crate) fn is_empty(&self) -> bool {
        self.files.is_empty()
    }

    pub(crate) fn files(&self) -> &[PathBuf] {
        &self.files
    }

    pub(crate) fn add(&mut self, paths: Vec<PathBuf>) {
        for path in paths {
            if !self.files.contains(&path) {
                self.files.push(path);
            }
        }
        self.files.sort_by_cached_key(|path| {
            path.file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_lowercase()
        });
    }

    pub(crate) fn remove(&mut self, path: &Path) {
        self.files.retain(|entry| entry != path);
    }
}

#[derive(Clone, Copy, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum AccentColor {
    #[default]
    Orange,
    Teal,
    Red,
    Pink,
    Purple,
    Yellow,
}
