pub(crate) mod actions;
mod bootstrap;
pub(crate) mod media_metadata;
pub(crate) mod project_controller;
#[cfg(feature = "self-update")]
mod update;

pub(crate) use bootstrap::run;

#[cfg(feature = "self-update")]
pub(crate) use update::initialize as initialize_updates;

#[cfg(not(feature = "self-update"))]
pub(crate) fn initialize_updates() {}
