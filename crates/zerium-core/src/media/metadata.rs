//! Native stream metadata and validation of reader results.
use super::MediaTarget;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use thiserror::Error;

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
        if self
            .kind
            .dimensions()
            .is_some_and(|[width, height]| width == 0 || height == 0)
        {
            return Err(MediaMetadataError::InvalidDimensions);
        }
        match self.kind {
            MediaKind::Video { frame_count: 0, .. } => {
                return Err(MediaMetadataError::EmptyVideo);
            }
            MediaKind::Audio {
                channels,
                sample_rate,
            } if channels == Some(0) || sample_rate == Some(0) => {
                return Err(MediaMetadataError::InvalidAudio);
            }
            _ => {}
        }
        Ok(())
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

/// One reader's interpretation of a file. A missing stream is a successful
/// probe, e.g. a video without audio.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct FileMediaMetadata {
    pub reader: String,
    pub target: MediaTarget,
    pub metadata: Option<MediaMetadata>,
}
