//! Persisted representations and checked reconstruction; filesystem I/O lives in engine::project_io.
mod clipboard;
mod error;
mod items;
mod paths;
mod project;
mod scenes;

pub use clipboard::{
    DecodedTimelineClipboard, decode_timeline_clipboard, encode_timeline_clipboard,
};
pub use error::ProjectError;
pub use project::{LoadedProject, PROJECT_EXTENSION, decode, encode};
