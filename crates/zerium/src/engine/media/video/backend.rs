//! Codec-specific I/O behind one seekable decoded-frame stream.
use super::gpu_decoder::GpuVideoDecoder;
use crate::engine::{
    gpu::GpuContext,
    gstreamer::input::{VideoInput, VideoInputMode},
    media::{DecodedFrame, MediaError},
};
use std::{sync::atomic::AtomicBool, time::Duration};
use zerium_core::media::MediaAsset;

pub(super) enum DecodeBackend {
    Gpu {
        input: VideoInput,
        decoder: GpuVideoDecoder,
    },
    Cpu(VideoInput),
}

impl DecodeBackend {
    pub(super) fn open(
        asset: &MediaAsset,
        gpu: Option<&GpuContext>,
        cancelled: &AtomicBool,
    ) -> Result<Self, MediaError> {
        match gpu {
            Some(gpu) => {
                let decoder = GpuVideoDecoder::new(gpu)?;
                let input = VideoInput::open(asset, VideoInputMode::H264, cancelled)?;
                Ok(Self::Gpu { input, decoder })
            }
            None => VideoInput::open(asset, VideoInputMode::Raw, cancelled).map(Self::Cpu),
        }
    }

    pub(super) fn is_gpu(&self) -> bool {
        matches!(self, Self::Gpu { .. })
    }

    pub(super) fn next(
        &mut self,
        cancelled: &AtomicBool,
    ) -> Result<Option<DecodedFrame>, MediaError> {
        match self {
            Self::Gpu { input, decoder } => decoder.next(input, cancelled),
            Self::Cpu(input) => input.read_frame(cancelled),
        }
    }

    pub(super) fn seek(&mut self, time: Duration) -> Result<(), MediaError> {
        match self {
            Self::Gpu { input, decoder } => {
                decoder.seek()?;
                input.seek(time)
            }
            Self::Cpu(input) => input.seek(time),
        }
    }
}
