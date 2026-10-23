//! Lazy decoder lifetime and the one-way GPU-to-CPU fallback policy.
use super::{backend::DecodeBackend, cursor::PlaybackCursor};
use crate::engine::{
    gpu::GpuContext,
    media::{DecodedFrame, MediaError, VideoDecoder},
};
use std::{
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};
use zerium_core::media::MediaAsset;

pub(crate) struct VideoDecodeSession {
    asset: MediaAsset,
    state: DecoderState,
}

enum DecoderState {
    Unopened { gpu: Option<GpuContext> },
    Open(PlaybackCursor),
}

impl DecoderState {
    fn is_gpu(&self) -> bool {
        match self {
            Self::Unopened { gpu } => gpu.is_some(),
            Self::Open(cursor) => cursor.is_gpu(),
        }
    }
}

impl VideoDecodeSession {
    pub(crate) fn new(asset: MediaAsset, gpu: Option<&GpuContext>) -> Self {
        Self {
            asset,
            state: DecoderState::Unopened { gpu: gpu.cloned() },
        }
    }

    fn decode_frame(
        &mut self,
        time: Duration,
        cancelled: &AtomicBool,
    ) -> Result<DecodedFrame, MediaError> {
        if cancelled.load(Ordering::Acquire) {
            return Err(MediaError::Cancelled);
        }
        match &mut self.state {
            DecoderState::Open(cursor) => cursor.frame_at(time, cancelled),
            DecoderState::Unopened { gpu } => {
                let backend = DecodeBackend::open(&self.asset, gpu.as_ref(), cancelled)?;
                let mut cursor = PlaybackCursor::new(backend, self.asset.duration);
                let result = cursor.frame_at(time, cancelled);
                self.state = DecoderState::Open(cursor);
                result
            }
        }
    }
}

impl VideoDecoder for VideoDecodeSession {
    fn decode_at(
        &mut self,
        time: Duration,
        cancelled: &AtomicBool,
    ) -> Result<DecodedFrame, MediaError> {
        match self.decode_frame(time, cancelled) {
            Err(MediaError::Cancelled) => Err(MediaError::Cancelled),
            Err(error) if self.state.is_gpu() => {
                log::warn!("GPU video decoding unavailable; using CPU decoding: {error}");
                // Replacing the state drops the backend and all presentation
                // history together. No GPU frames survive into the CPU cursor.
                self.state = DecoderState::Unopened { gpu: None };
                self.decode_frame(time, cancelled)
            }
            result => result,
        }
    }
}
