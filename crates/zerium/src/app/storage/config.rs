//! User-selected settings. Unspecified options follow the application's defaults.

use serde::{Deserialize, Serialize};

use super::JsonStore;
use crate::app::defaults::CreationDefaults;

pub(crate) type ConfigStore = JsonStore<AppConfig>;

#[derive(Clone, Default, Deserialize, Serialize)]
#[serde(default)]
pub(crate) struct AppConfig {
    #[serde(skip_serializing_if = "CreationDefaults::is_empty")]
    pub(crate) creation_defaults: CreationDefaults,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) language: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) show_support_reminder: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) theme_mode: Option<ui::ThemeMode>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) accent_color: Option<AccentColor>,
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
