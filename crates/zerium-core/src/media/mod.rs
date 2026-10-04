mod playback;

pub use playback::{MediaEndBehavior, MediaPlayback, MediaSample};

use crate::property::PropertyValues;
use std::{
    collections::{BTreeMap, HashSet},
    path::{Path, PathBuf},
    time::Duration,
};

use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MediaTarget {
    Visual,
    Audio,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Hash, Serialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum MediaKind {
    Video {
        width: u32,
        height: u32,
        frame_rate: VideoFrameRate,
        frame_count: u64,
    },
    Audio {
        channels: Option<u32>,
        sample_rate: Option<u32>,
    },
    Image {
        width: u32,
        height: u32,
    },
}

/// The native cadence of a video stream.
///
/// This belongs to the media asset rather than the timeline: a timeline frame
/// is mapped to a native video frame by time before decoding or caching it.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Hash, Serialize)]
#[serde(try_from = "[u32; 2]", into = "[u32; 2]")]
pub struct VideoFrameRate {
    numerator: u32,
    denominator: u32,
}

impl VideoFrameRate {
    pub const fn new(numerator: u32, denominator: u32) -> Option<Self> {
        if numerator == 0 || denominator == 0 {
            return None;
        }
        Some(Self {
            numerator,
            denominator,
        })
    }

    pub fn frames_per_second(self) -> f64 {
        f64::from(self.numerator) / f64::from(self.denominator)
    }

    pub fn frame_to_seconds(self, frame: u64) -> f64 {
        frame as f64 * f64::from(self.denominator) / f64::from(self.numerator)
    }

    pub const fn numerator(self) -> u32 {
        self.numerator
    }

    pub const fn denominator(self) -> u32 {
        self.denominator
    }
}

impl TryFrom<[u32; 2]> for VideoFrameRate {
    type Error = &'static str;

    fn try_from([numerator, denominator]: [u32; 2]) -> Result<Self, Self::Error> {
        Self::new(numerator, denominator).ok_or("video frame rate must be positive")
    }
}

impl From<VideoFrameRate> for [u32; 2] {
    fn from(frame_rate: VideoFrameRate) -> Self {
        [frame_rate.numerator, frame_rate.denominator]
    }
}

impl MediaKind {
    pub const fn is_temporal(&self) -> bool {
        matches!(self, Self::Video { .. } | Self::Audio { .. })
    }

    pub const fn dimensions(&self) -> Option<[u32; 2]> {
        match self {
            Self::Video { width, height, .. } | Self::Image { width, height } => {
                Some([*width, *height])
            }
            Self::Audio { .. } => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct MediaAsset {
    pub reader_id: String,
    pub path: PathBuf,
    pub duration: Duration,
    pub kind: MediaKind,
}

#[derive(Clone, Copy, Debug, Error, PartialEq, Eq)]
pub enum MediaMetadataError {
    #[error("temporal media duration must be positive")]
    EmptyDuration,
    #[error("visual media dimensions must be non-zero")]
    InvalidDimensions,
    #[error("video frame count must be non-zero")]
    EmptyVideo,
    #[error("audio channel count and sample rate must be non-zero")]
    InvalidAudio,
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

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct MediaMetadata {
    pub duration: Duration,
    pub kind: MediaKind,
}

impl MediaMetadata {
    pub fn validate(&self) -> Result<(), MediaMetadataError> {
        if self.kind.is_temporal() && self.duration.is_zero() {
            return Err(MediaMetadataError::EmptyDuration);
        }
        match self.kind {
            MediaKind::Video {
                width,
                height,
                frame_count,
                ..
            } => {
                if width == 0 || height == 0 {
                    return Err(MediaMetadataError::InvalidDimensions);
                }
                if frame_count == 0 {
                    return Err(MediaMetadataError::EmptyVideo);
                }
            }
            MediaKind::Image { width, height } if width == 0 || height == 0 => {
                return Err(MediaMetadataError::InvalidDimensions);
            }
            MediaKind::Audio {
                channels,
                sample_rate,
            } if channels == Some(0) || sample_rate == Some(0) => {
                return Err(MediaMetadataError::InvalidAudio);
            }
            MediaKind::Audio { .. } | MediaKind::Image { .. } => {}
        }
        Ok(())
    }

    fn asset(&self, path: &Path, reader: &str) -> MediaAsset {
        MediaAsset {
            reader_id: reader.to_owned(),
            path: path.to_owned(),
            duration: self.duration,
            kind: self.kind.clone(),
        }
    }

    pub fn accepts(&self, target: MediaTarget) -> bool {
        matches!(
            (target, &self.kind),
            (
                MediaTarget::Visual,
                MediaKind::Video { .. } | MediaKind::Image { .. }
            ) | (MediaTarget::Audio, MediaKind::Audio { .. })
        ) && self.validate().is_ok()
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct MediaInputMetadata {
    pub input: MediaInputReference,
    /// A missing stream is valid, e.g. a video without an audio track.
    pub metadata: Option<MediaMetadata>,
}

/// Derived input metadata. File paths and readers are always resolved from
/// properties and capabilities. The metadata is saved for offline editing.
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(try_from = "Vec<MediaInputMetadata>", into = "Vec<MediaInputMetadata>")]
pub struct MediaInputs(BTreeMap<MediaInputReference, MediaMetadata>);

impl TryFrom<Vec<MediaInputMetadata>> for MediaInputs {
    type Error = &'static str;

    fn try_from(inputs: Vec<MediaInputMetadata>) -> Result<Self, Self::Error> {
        let mut result = Self::default();
        for input in inputs {
            let metadata = input
                .metadata
                .ok_or("persisted media input metadata must not be null")?;
            if !metadata.accepts(input.input.target()) {
                return Err("invalid media input metadata");
            }
            if result.0.insert(input.input, metadata).is_some() {
                return Err("duplicate media input metadata");
            }
        }
        Ok(result)
    }
}

impl From<MediaInputs> for Vec<MediaInputMetadata> {
    fn from(inputs: MediaInputs) -> Self {
        inputs
            .0
            .into_iter()
            .map(|(input, metadata)| MediaInputMetadata {
                input,
                metadata: Some(metadata),
            })
            .collect()
    }
}

impl MediaInputs {
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn asset(
        &self,
        source: MediaSource<'_>,
        properties: &PropertyValues,
    ) -> Option<MediaAsset> {
        let path = properties.property(source.file)?.file()?;
        Some(self.0.get(&source.input)?.asset(path, source.reader))
    }

    pub fn validate<'a>(
        &self,
        sources: impl Iterator<Item = MediaSource<'a>>,
        properties: &PropertyValues,
    ) -> bool {
        let sources = sources.collect::<Vec<_>>();
        // Metadata is validated on construction/import; only ownership can change.
        self.0.keys().all(|input| {
            sources.iter().any(|source| {
                source.input == *input
                    && properties
                        .property(source.file)
                        .and_then(|value| value.file())
                        .is_some()
            })
        })
    }

    pub fn invalidate_file<'a>(
        &mut self,
        sources: impl Iterator<Item = MediaSource<'a>>,
        property: &str,
    ) {
        for source in sources.filter(|source| source.file == property) {
            self.0.remove(&source.input);
        }
    }

    /// Replace all interpretations of one file as a single validated operation.
    pub fn replace_file<'a>(
        &mut self,
        sources: impl Iterator<Item = MediaSource<'a>>,
        property: &str,
        inputs: Vec<MediaInputMetadata>,
    ) -> bool {
        let mut remaining = sources
            .filter(|source| source.file == property)
            .map(|source| source.input)
            .collect::<HashSet<_>>();
        if inputs.iter().any(|input| {
            !remaining.remove(&input.input)
                || input
                    .metadata
                    .as_ref()
                    .is_some_and(|metadata| !metadata.accepts(input.input.target()))
        }) || !remaining.is_empty()
        {
            return false;
        }
        for input in inputs {
            match input.metadata {
                Some(metadata) => {
                    self.0.insert(input.input, metadata);
                }
                None => {
                    self.0.remove(&input.input);
                }
            }
        }
        true
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImportedFile {
    pub plugin_id: String,
    pub source_id: String,
    pub property_id: String,
    pub path: PathBuf,
    pub inputs: Vec<MediaInputMetadata>,
}

impl ImportedFile {
    /// The complete temporal extent of a newly imported file. Static inputs do
    /// not extend the clip. This is an initial value, not an editing constraint.
    pub fn initial_duration(&self) -> Option<Duration> {
        self.inputs
            .iter()
            .filter_map(|input| input.metadata.as_ref())
            .filter(|metadata| metadata.kind.is_temporal())
            .map(|metadata| metadata.duration)
            .max()
    }
}
