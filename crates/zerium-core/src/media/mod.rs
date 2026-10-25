//! Media assets, reader metadata, and source playback.
mod metadata;
mod playback;

pub use self::metadata::{
    FileMediaMetadata, MediaKind, MediaMetadata, MediaMetadataCache, MediaMetadataError,
    MediaRevision, ProbedFile, VideoFrameRate,
};
pub use playback::{MediaEndBehavior, MediaPlayback, MediaSample};

use crate::property::PropertyValues;
use std::{path::PathBuf, time::Duration};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MediaTarget {
    Visual,
    Audio,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct MediaAsset {
    pub reader_id: String,
    pub path: PathBuf,
    pub revision: MediaRevision,
    pub duration: Duration,
    pub kind: MediaKind,
}

/// A file and the reader interpretation needed to discover its metadata.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct MediaDependency {
    pub path: PathBuf,
    pub reader_id: String,
    pub target: MediaTarget,
}

/// Identifies an interpreted input, independently of its file property.
#[derive(Clone, Debug, serde::Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
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
    pub frame_rate: Option<&'a str>,
}

impl MediaSource<'_> {
    pub fn asset(
        &self,
        properties: &PropertyValues,
        cache: &MediaMetadataCache,
    ) -> Option<MediaAsset> {
        let mut asset = cache.asset(
            properties.property(self.file)?.file()?,
            self.reader,
            self.input.target(),
        )?;
        self.apply_frame_rate(&mut asset, properties)?;
        Some(asset)
    }

    pub(crate) fn apply_frame_rate(
        &self,
        asset: &mut MediaAsset,
        properties: &PropertyValues,
    ) -> Option<()> {
        if let Some(property) = self.frame_rate
            && let MediaKind::ImageSequence {
                frame_rate, frames, ..
            } = &mut asset.kind
        {
            let fps = properties.property(property)?.as_f32()?;
            if !fps.is_finite() || !(0.001..=1000.).contains(&fps) {
                return None;
            }
            *frame_rate = VideoFrameRate::from_fps(fps)?;
            asset.duration =
                Duration::from_secs_f64(frame_rate.frame_to_seconds(frames.len() as u64));
        }
        Some(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImportedFile {
    pub plugin_id: String,
    pub source_id: String,
    pub property_id: String,
    pub file: ProbedFile,
}
