//! Media assets, reader metadata, and source playback.
mod metadata;
mod metadata_cache;
mod playback;

pub use metadata::{
    FileMediaMetadata, MediaKind, MediaMetadata, MediaMetadataError, VideoFrameRate,
};
pub use metadata_cache::{FileRevision, MediaMetadataCache, ProbedFile};
pub use playback::{MediaEndBehavior, MediaPlayback, MediaSample};

use crate::property::PropertyValues;
use serde::{Deserialize, Serialize};
use std::{path::PathBuf, time::Duration};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MediaTarget {
    Visual,
    Audio,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct MediaAsset {
    pub reader_id: String,
    pub path: PathBuf,
    pub revision: Option<FileRevision>,
    pub duration: Duration,
    pub kind: MediaKind,
}

/// Identifies an interpreted input, independently of its file property.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(
    tag = "type",
    content = "id",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum MediaInputReference {
    Media(String),
    Audio(String),
}

impl MediaInputReference {
    pub const fn target(&self) -> MediaTarget {
        match self {
            Self::Media(_) => MediaTarget::Visual,
            Self::Audio(_) => MediaTarget::Audio,
        }
    }
}

/// The reader and file selected by a capability, rather than by the file type.
#[derive(Clone, Debug)]
pub struct MediaSource<'a> {
    pub input: MediaInputReference,
    pub file: &'a str,
    pub reader: &'a str,
}

impl MediaSource<'_> {
    pub fn asset(
        &self,
        properties: &PropertyValues,
        cache: &MediaMetadataCache,
    ) -> Option<MediaAsset> {
        cache.asset(
            properties.property(self.file)?.file()?,
            self.reader,
            self.input.target(),
        )
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImportedFile {
    pub plugin_id: String,
    pub source_id: String,
    pub property_id: String,
    pub file: ProbedFile,
}
