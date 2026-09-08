//! Domain models and operations, grouped by the concepts they own.
//!
//! `parameter` is the shared contract/value layer. `animation` interprets those
//! values as tracks, and `plugin` adapts contracts to manifests and shader ABIs.
//! `timeline` coordinates editing and scene evaluation. `persistence` converts
//! domain state to/from project and clipboard representations; filesystem I/O
//! remains outside this module in `engine::project_io`.

pub(crate) mod animation;
pub(crate) mod media;
pub(crate) mod parameter;
pub(crate) mod persistence;
pub(crate) mod plugin;
pub(crate) mod timeline;
