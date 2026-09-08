use std::{error::Error, fmt, path::PathBuf, time::Duration};

use crate::domain::plugin::MediaType;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) enum MediaKind {
    Video {
        width: u32,
        height: u32,
        frame_rate: VideoFrameRate,
        frame_count: u64,
        has_audio: bool,
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
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct VideoFrameRate {
    numerator: u32,
    denominator: u32,
}

impl VideoFrameRate {
    pub(crate) const fn new(numerator: u32, denominator: u32) -> Option<Self> {
        if numerator == 0 || denominator == 0 {
            return None;
        }
        Some(Self {
            numerator,
            denominator,
        })
    }

    pub(crate) fn frames_per_second(self) -> f64 {
        f64::from(self.numerator) / f64::from(self.denominator)
    }

    pub(crate) fn frame_to_seconds(self, frame: u64) -> f64 {
        frame as f64 * f64::from(self.denominator) / f64::from(self.numerator)
    }

    pub(crate) const fn numerator(self) -> u32 {
        self.numerator
    }

    pub(crate) const fn denominator(self) -> u32 {
        self.denominator
    }
}

impl MediaKind {
    pub(crate) const fn media_type(&self) -> MediaType {
        match self {
            Self::Video { .. } => MediaType::Video,
            Self::Audio { .. } => MediaType::Audio,
            Self::Image { .. } => MediaType::Image,
        }
    }

    pub(crate) const fn has_audio(&self) -> bool {
        match self {
            Self::Video { has_audio, .. } => *has_audio,
            Self::Audio { .. } => true,
            Self::Image { .. } => false,
        }
    }

    pub(crate) const fn dimensions(&self) -> Option<[u32; 2]> {
        match self {
            Self::Video { width, height, .. } | Self::Image { width, height } => {
                Some([*width, *height])
            }
            Self::Audio { .. } => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct MediaAsset {
    pub reader_id: String,
    pub path: PathBuf,
    pub name: String,
    pub duration: Duration,
    pub kind: MediaKind,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct MediaSourceId {
    reader_id: String,
    path: PathBuf,
    duration: Duration,
    kind: MediaKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum MediaMetadataError {
    EmptyReader,
    InvalidDimensions,
    EmptyVideo,
    InvalidAudio,
}

impl fmt::Display for MediaMetadataError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::EmptyReader => "media reader ID must not be empty",
            Self::InvalidDimensions => "visual media dimensions must be non-zero",
            Self::EmptyVideo => "video frame count must be non-zero",
            Self::InvalidAudio => "audio channel count and sample rate must be non-zero",
        })
    }
}

impl Error for MediaMetadataError {}

impl MediaAsset {
    pub(crate) fn validate(&self) -> Result<(), MediaMetadataError> {
        if self.reader_id.trim().is_empty() {
            return Err(MediaMetadataError::EmptyReader);
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

    pub(crate) fn source_id(&self) -> MediaSourceId {
        MediaSourceId {
            reader_id: self.reader_id.clone(),
            path: self.path.clone(),
            duration: self.duration,
            kind: self.kind.clone(),
        }
    }

    /// Maps an item-local timeline time into this asset. Media items loop when
    /// their timeline duration extends beyond the source duration.
    pub(crate) fn looped_seconds(&self, local_seconds: f64) -> f64 {
        let duration = self.duration.as_secs_f64();
        if !duration.is_finite() || duration <= 0. {
            return 0.;
        }
        local_seconds.max(0.).rem_euclid(duration)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ImportedMedia {
    pub plugin_id: String,
    pub item_id: String,
    pub input_id: String,
    pub asset: MediaAsset,
}
