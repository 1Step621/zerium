//! Automatically recorded application state, persisted separately from user settings.

use serde::{Deserialize, Serialize};

use super::persistence::JsonStore;

pub(crate) type StateStore = JsonStore<AppState>;

#[derive(Clone, Default, Deserialize, Serialize)]
#[serde(default)]
pub(crate) struct AppState {
    pub(crate) support_reminder: SupportReminderState,
}

#[derive(Clone, Default, Deserialize, Serialize)]
#[serde(default)]
pub(crate) struct SupportReminderState {
    pub(crate) launches_since_reminder: u64,
    pub(crate) last_shown_at: Option<u64>,
}
