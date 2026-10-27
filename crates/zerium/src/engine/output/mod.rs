//! Audio and visual output, with common stream encoding contracts.
pub(crate) mod audio;
pub(crate) mod visual;
use super::input::AudioFormat;
use thiserror::Error;
use zerium_core::media::VideoFrameRate;
use zerium_core::timeline::{
    AudioCodec, ExportBitDepth, ExportColorSettings, ExportContainer, VideoCodec,
};

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
        // Raw-video caps and aligned plane strides use signed 32-bit dimensions.
        if self.width > i32::MAX as u32 / 4 || self.height > i32::MAX as u32 {
            return Err(EncodingError::Invalid("Video dimensions are too large"));
        }
        Ok(self)
    }
}

#[derive(Clone, Copy)]
pub(crate) struct EncodingSettings {
    pub(crate) color: ExportColorSettings,
    pub(crate) container: ExportContainer,
    pub(crate) gif_loop: bool,
    pub(crate) video_codec: VideoCodec,
    pub(crate) audio_codec: AudioCodec,
    pub(crate) video_bitrate_kbps: Option<u32>,
    pub(crate) audio_bitrate_kbps: u32,
    pub(crate) output: VideoDimensions,
    pub(crate) frame_rate: VideoFrameRate,
    pub(crate) gop: u32,
    pub(crate) audio: Option<AudioFormat>,
}

impl EncodingSettings {
    pub(crate) fn pixel_format(self) -> VideoPixelFormat {
        if self.container == ExportContainer::Gif {
            VideoPixelFormat::Rgba
        } else {
            self.color.depth.into()
        }
    }
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

/// Raw pixel layout exchanged between rendering and encoding.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum VideoPixelFormat {
    Nv12,
    P010,
    Rgba,
}

impl From<ExportBitDepth> for VideoPixelFormat {
    fn from(depth: ExportBitDepth) -> Self {
        match depth {
            ExportBitDepth::Eight => Self::Nv12,
            ExportBitDepth::Ten => Self::P010,
        }
    }
}

impl VideoPixelFormat {
    /// Unpadded byte length of a plane row for the given frame width.
    pub(crate) fn row_bytes(self, width: u32) -> u32 {
        match self {
            Self::Nv12 => width,
            Self::P010 => width * 2,
            Self::Rgba => width * 4,
        }
    }
}

/// Pixel or bitstream input accepted by the media output adapter.
#[derive(Clone, Copy)]
pub(crate) enum VideoEncoderInput {
    Raw(VideoPixelFormat),
    Encoded,
}

pub(crate) struct VideoEncoderFrame {
    pub(crate) index: u64,
    pub(crate) data: Vec<u8>,
    pub(crate) keyframe: bool,
}
