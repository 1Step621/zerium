pub(crate) mod actions;
mod bootstrap;
pub(crate) mod config;
mod gpu;
pub(crate) mod media_metadata;
mod persistence;
pub(crate) mod project_controller;
pub(crate) mod state;
#[cfg(feature = "self-update")]
mod update;
#[cfg(not(feature = "self-update"))]
mod update {
    pub(crate) fn initialize() {}

    pub(crate) fn start(_cx: &mut gpui::App) {}
}

pub(crate) use bootstrap::run;
pub(crate) use update::{initialize as initialize_updates, start as start_updates};
