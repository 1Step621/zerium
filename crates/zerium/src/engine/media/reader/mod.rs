//! Shared decoder contracts, media input IDs, and errors.
use crate::engine::gpu::GpuContext;
use std::{fmt, path::Path, sync::atomic::AtomicBool, time::Duration};

use thiserror::Error;

use crate::engine::frame::VisualFrame;
use zerium_core::{
    media::{MediaAsset, MediaMetadata, MediaTarget},
    timeline::{EffectInstanceId, ItemId},
};

mod registry;

pub(crate) use registry::{MediaReaderRegistry, bundled_media_readers};

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct MediaInputId {
    pub(crate) item_id: ItemId,
    pub(crate) effect_id: Option<EffectInstanceId>,
    pub(crate) input_id: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct AudioFormat {
    pub sample_rate: u32,
    pub channels: u16,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct DecodedAudioBlock {
    pub format: AudioFormat,
    /// Interleaved `f32` samples in channel order.
    pub samples: Vec<f32>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct RasterSize {
    pub max_width: u32,
    pub max_height: u32,
}

impl RasterSize {
    pub(super) fn fit(self, (source_width, source_height): (u32, u32)) -> Option<(u32, u32)> {
        if source_width == 0 || source_height == 0 || self.max_width == 0 || self.max_height == 0 {
            return None;
        }
        let scale = (f64::from(self.max_width) / f64::from(source_width))
            .min(f64::from(self.max_height) / f64::from(source_height))
            .min(1.);
        Some((
            (f64::from(source_width) * scale).round().max(1.) as u32,
            (f64::from(source_height) * scale).round().max(1.) as u32,
        ))
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct DecodedFrame {
    pub presentation_time: Duration,
    pub duration: Duration,
    pub frame: VisualFrame,
}

impl DecodedFrame {
    pub(crate) fn covers(&self, presentation_time: Duration) -> bool {
        let rounded = presentation_time.saturating_add(Duration::from_nanos(1));
        self.presentation_time <= rounded
            && rounded < self.presentation_time.saturating_add(self.duration)
    }
}

pub(crate) trait VideoDecoder: Send {
    /// Returns the frame whose presentation interval contains `presentation_time`,
    /// or the closest following frame when the stream has a timestamp gap.
    fn decode_at(
        &mut self,
        presentation_time: Duration,
        cancelled: &AtomicBool,
    ) -> Result<DecodedFrame, MediaError>;
}

pub(crate) trait ImageDecoder: Send {
    fn render(
        &mut self,
        size: RasterSize,
        cancelled: &AtomicBool,
    ) -> Result<VisualFrame, MediaError>;
}

pub(crate) trait AudioDecoder: Send {
    fn stream_duration(&self) -> Duration;

    /// Configure output speed before reading; seeking uses source time.
    fn set_playback_rate(&mut self, speed: f64, preserve_pitch: bool) -> Result<(), MediaError>;

    /// Decodes `sample_frames` output frames at the configured playback rate.
    /// `start_seconds` is in source time; one frame has one sample per channel.
    /// A short or empty block indicates the end of the stream.
    fn decode_sample_frames(
        &mut self,
        start_seconds: f64,
        sample_frames: usize,
        format: AudioFormat,
    ) -> Result<DecodedAudioBlock, MediaError>;
}

pub(crate) trait MediaReader: Send + Sync {
    fn probe(&self, path: &Path, target: MediaTarget) -> Result<Option<MediaMetadata>, MediaError>;

    /// GPU decoding uses the consumer's device and queue. Without them, or when
    /// the GPU cannot decode the stream, the decoder produces CPU pixels.
    fn open_video_decoder(
        &self,
        asset: &MediaAsset,
        gpu: Option<&GpuContext>,
    ) -> Result<Box<dyn VideoDecoder>, MediaError> {
        let _ = (asset, gpu);
        Err(MediaError::unsupported(
            "This media reader does not support video decoding",
        ))
    }

    fn open_image_decoder(&self, asset: &MediaAsset) -> Result<Box<dyn ImageDecoder>, MediaError> {
        let _ = asset;
        Err(MediaError::unsupported(
            "This media reader does not support still image decoding",
        ))
    }

    fn open_audio_decoder(&self, asset: &MediaAsset) -> Result<Box<dyn AudioDecoder>, MediaError> {
        let _ = asset;
        Err(MediaError::unsupported(
            "This media reader does not support audio decoding",
        ))
    }
}

#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub(crate) enum MediaError {
    #[error("{0}")]
    Unsupported(String),
    #[error("{0}")]
    InvalidInput(String),
    #[error("{0}")]
    ReaderUnavailable(String),
    #[error("{0}")]
    External(String),
    #[error("Operation was cancelled")]
    Cancelled,
}

impl MediaError {
    pub(in crate::engine) fn external(message: impl fmt::Display) -> Self {
        Self::External(message.to_string())
    }

    pub(super) fn unsupported(message: impl fmt::Display) -> Self {
        Self::Unsupported(message.to_string())
    }

    pub(in crate::engine) fn invalid_input(message: impl fmt::Display) -> Self {
        Self::InvalidInput(message.to_string())
    }
}
