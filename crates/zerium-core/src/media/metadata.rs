//! Native stream metadata and validation of reader results.
use super::MediaTarget;
use std::{path::PathBuf, sync::Arc, time::Duration};
use thiserror::Error;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum MediaKind {
    Video {
        width: u32,
        height: u32,
        /// Nominal cadence, if reported by the stream. Variable-rate streams
        /// may omit it; decoded timestamps define their frame intervals.
        frame_rate: Option<VideoFrameRate>,
        /// Estimated count when a nominal cadence is available.
        frame_count: Option<u64>,
    },
    Audio {
        channels: Option<u32>,
        sample_rate: Option<u32>,
    },
    Image {
        width: u32,
        height: u32,
    },
    ImageSequence {
        width: u32,
        height: u32,
        frame_rate: VideoFrameRate,
        /// File names relative to the source directory at the observed revision.
        frames: Arc<[PathBuf]>,
    },
}

/// The nominal cadence reported by a video stream.
///
/// This belongs to the media asset rather than the timeline. Actual decoded
/// timestamps determine which video frame covers a requested time.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
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

    /// Recover a simple rational from an authored f32, including 30000/1001.
    /// Using a rounded decimal cadence would shift exact timeline boundaries.
    pub(super) fn from_fps(fps: f32) -> Option<Self> {
        if !fps.is_finite() || fps <= 0. {
            return None;
        }
        let mut remainder = f64::from(fps);
        let (mut n0, mut n1) = (0_u64, 1_u64);
        let (mut d0, mut d1) = (1_u64, 0_u64);
        loop {
            let whole = remainder.floor() as u64;
            let n = whole.checked_mul(n1)?.checked_add(n0)?;
            let d = whole.checked_mul(d1)?.checked_add(d0)?;
            let rate = Self::new(u32::try_from(n).ok()?, u32::try_from(d).ok()?);
            if let Some(rate) = rate
                && rate.frames_per_second() as f32 == fps
            {
                return Some(rate);
            }
            let fraction = remainder - whole as f64;
            if fraction == 0. {
                return None;
            }
            remainder = 1. / fraction;
            (n0, n1) = (n1, n);
            (d0, d1) = (d1, d);
        }
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

impl MediaKind {
    pub const fn is_temporal(&self) -> bool {
        matches!(
            self,
            Self::Video { .. } | Self::Audio { .. } | Self::ImageSequence { .. }
        )
    }

    pub const fn dimensions(&self) -> Option<[u32; 2]> {
        match self {
            Self::Video { width, height, .. }
            | Self::Image { width, height }
            | Self::ImageSequence { width, height, .. } => Some([*width, *height]),
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
    #[error("image sequences require unique file names without directory components")]
    InvalidImageSequence,
    #[error("audio channel count and sample rate must be non-zero")]
    InvalidAudio,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MediaMetadata {
    /// Native source duration. Still images have no time axis and report zero;
    /// the document decides their initial placement length.
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
            MediaKind::ImageSequence { ref frames, .. } => {
                let mut seen = std::collections::HashSet::new();
                if frames.is_empty()
                    || !frames.iter().all(|path| {
                        let mut components = path.components();
                        matches!(components.next(), Some(std::path::Component::Normal(_)))
                            && components.next().is_none()
                            && seen.insert(path)
                    })
                {
                    return Err(MediaMetadataError::InvalidImageSequence);
                }
            }
            MediaKind::Video {
                frame_count: Some(0),
                ..
            } => {
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
                MediaKind::Video { .. } | MediaKind::Image { .. } | MediaKind::ImageSequence { .. }
            ) | (MediaTarget::Audio, MediaKind::Audio { .. })
        ) && self.validate().is_ok()
    }
}

/// One reader's interpretation of a file. A missing stream is a successful
/// probe, e.g. a video without audio.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileMediaMetadata {
    pub reader: String,
    pub target: MediaTarget,
    pub revision: super::MediaRevision,
    pub metadata: Option<MediaMetadata>,
}
