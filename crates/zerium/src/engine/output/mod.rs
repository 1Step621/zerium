//! Audio and visual output, with common stream encoding contracts.
pub(crate) mod audio;
pub(crate) mod visual;
use super::input::AudioFormat;
use thiserror::Error;
use zerium_core::media::VideoFrameRate;

#[derive(Clone, Copy, Debug)]
pub(crate) struct VideoDimensions {
    pub(crate) width: u32,
    pub(crate) height: u32,
}

impl VideoDimensions {
    pub(crate) fn validate(self) -> Result<Self, EncodingError> {
        if self.width == 0 || self.height == 0 {
            return Err(EncodingError::Invalid("Video dimensions must be positive"));
        }
        if !self.width.is_multiple_of(2) || !self.height.is_multiple_of(2) {
            return Err(EncodingError::Invalid(
                "YUV420 output requires even dimensions",
            ));
        }
        // Raw-video caps and aligned plane strides use signed 32-bit dimensions.
        if self.width > i32::MAX as u32 / 4 || self.height > i32::MAX as u32 {
            return Err(EncodingError::Invalid("Video dimensions are too large"));
        }
        Ok(self)
    }
}

#[derive(Clone, Copy)]
pub(crate) struct EncodingSettings {
    pub(crate) output: VideoDimensions,
    pub(crate) frame_rate: VideoFrameRate,
    pub(crate) gop: u32,
    pub(crate) audio: Option<AudioFormat>,
}

#[derive(Debug, Error)]
pub(crate) enum EncodingError {
    #[error("Encoding cancelled")]
    Cancelled,
    #[error("{0}")]
    Invalid(&'static str),
    #[error("Encoding failed: {0}")]
    Backend(String),
    #[error("Timed out waiting for the encoder")]
    Timeout,
    #[error("MP4 audio timing could not be finalized: {0}")]
    Finalize(#[from] mp4::Error),
}

impl EncodingError {
    pub(crate) fn backend(error: impl std::fmt::Display) -> Self {
        Self::Backend(error.to_string())
    }
}

/// Pixel or bitstream input accepted by the MP4 output adapter.
pub(crate) enum VideoEncoderInput {
    Nv12,
    H264,
}

pub(crate) struct VideoEncoderFrame {
    pub(crate) index: u64,
    pub(crate) data: Vec<u8>,
    pub(crate) keyframe: bool,
}
