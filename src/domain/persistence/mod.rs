//! Persisted representations and checked reconstruction; filesystem I/O lives in engine::project_io.
mod clipboard;
mod error;
mod project;

pub(crate) use clipboard::{
    DecodedTimelineClipboard, decode_timeline_clipboard, encode_timeline_clipboard,
};
pub(crate) use error::ProjectError;
pub(crate) use project::{LoadedProject, PROJECT_EXTENSION, decode, encode};
