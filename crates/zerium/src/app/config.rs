//! User-selected settings. Unspecified options follow the application's defaults.

use serde::{Deserialize, Serialize};

use super::persistence::JsonStore;

pub(crate) type ConfigStore = JsonStore<AppConfig>;

#[derive(Clone, Default, Deserialize, Serialize)]
#[serde(default)]
pub(crate) struct AppConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) language: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) show_support_reminder: Option<bool>,
}
