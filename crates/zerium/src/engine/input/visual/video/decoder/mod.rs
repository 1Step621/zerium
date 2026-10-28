//! Decoder lifetime, presentation intervals, and GPU-to-CPU fallback.
use std::{
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};
use zerium_core::media::{MediaAsset, MediaKind};
use {
    self::{
        gpu_video::GpuVideoDecoder,
        gstreamer::{VideoInput, VideoInputMode},
    },
    crate::engine::{
        GpuContext,
        input::{DecodedFrame, MediaError, VideoDecoder},
    },
};

mod gpu_video;
mod gstreamer;

pub(super) struct VideoDecodeSession {
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
    pub(super) fn new(asset: MediaAsset, gpu: Option<&GpuContext>) -> Self {
        // The native GPU backend only supports H.264. Select the CPU backend
        // directly for other formats instead of opening a pipeline that must fail.
        let gpu = match &asset.kind {
            MediaKind::Video { codec, .. } if codec.as_deref() == Some("video/x-h264") => {
                gpu.cloned()
            }
            _ => None,
        };
        Self {
            asset,
            state: DecoderState::Unopened { gpu },
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

enum DecodeBackend {
    Gpu {
        input: VideoInput,
        decoder: GpuVideoDecoder,
    },
    Cpu(VideoInput),
}

impl DecodeBackend {
    fn open(
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

    fn is_gpu(&self) -> bool {
        matches!(self, Self::Gpu { .. })
    }

    fn next(&mut self, cancelled: &AtomicBool) -> Result<Option<DecodedFrame>, MediaError> {
        match self {
            Self::Gpu { input, decoder } => decoder.next(input, cancelled),
            Self::Cpu(input) => input.read_frame(cancelled),
        }
    }

    fn seek(&mut self, time: Duration) -> Result<(), MediaError> {
        match self {
            Self::Gpu { input, decoder } => {
                decoder.seek()?;
                input.seek(time)
            }
            Self::Cpu(input) => input.seek(time),
        }
    }
}

// Consume nearby forward requests sequentially; distant requests use keyframe seeks.
const MAX_FORWARD_DECODE_FRAMES: u32 = 10;

struct PlaybackCursor {
    backend: DecodeBackend,
    duration: Duration,
    position: CursorPosition,
    lookahead: Option<DecodedFrame>,
}

/// The backend can move before the first presentation interval is complete.
/// A cancelled read must not make a seeked stream look newly opened at zero.
enum CursorPosition {
    Unknown,
    StartedAt(Duration),
    Frame(DecodedFrame),
}

impl PlaybackCursor {
    fn new(backend: DecodeBackend, duration: Duration) -> Self {
        Self {
            backend,
            duration,
            position: CursorPosition::StartedAt(Duration::ZERO),
            lookahead: None,
        }
    }

    fn is_gpu(&self) -> bool {
        self.backend.is_gpu()
    }

    fn seek(&mut self, time: Duration) -> Result<(), MediaError> {
        self.position = CursorPosition::Unknown;
        self.lookahead = None;
        self.backend.seek(time)?;
        self.position = CursorPosition::StartedAt(time);
        Ok(())
    }

    /// The following PTS defines the current frame's actual presentation end.
    /// Nominal duration can overlap the next frame after time-base rounding,
    /// and accurate seeks can produce two samples at the same clipped PTS.
    fn read_interval(
        &mut self,
        cancelled: &AtomicBool,
    ) -> Result<Option<DecodedFrame>, MediaError> {
        let mut current = match self.lookahead.take() {
            Some(frame) => frame,
            None => {
                let Some(frame) = self.backend.next(cancelled)? else {
                    return Ok(None);
                };
                frame
            }
        };
        loop {
            let next = match self.backend.next(cancelled) {
                Ok(next) => next,
                Err(error) => {
                    // The current picture has not been presented yet. Keep it
                    // if reading its interval end is interrupted, so resuming
                    // does not skip a frame.
                    self.lookahead = Some(current);
                    return Err(error);
                }
            };
            let Some(next) = next else {
                // A container can keep playing audio after video ends. The
                // final picture covers that tail, including requests after a seek.
                current.duration = current
                    .duration
                    .max(self.duration.saturating_sub(current.presentation_time));
                return Ok(Some(current));
            };
            if next.presentation_time <= current.presentation_time {
                current = next;
                continue;
            }
            current.duration = next
                .presentation_time
                .saturating_sub(current.presentation_time);
            self.lookahead = Some(next);
            return Ok(Some(current));
        }
    }

    fn frame_at(
        &mut self,
        time: Duration,
        cancelled: &AtomicBool,
    ) -> Result<DecodedFrame, MediaError> {
        if cancelled.load(Ordering::Relaxed) {
            return Err(MediaError::Cancelled);
        }
        // Nearby forward requests consume the existing stream. Backward and
        // distant requests decode from the preceding keyframe to the target.
        let (seek, previous) = match &self.position {
            CursorPosition::Frame(last) if last.covers(time) => return Ok(last.clone()),
            CursorPosition::Frame(last) => (
                time < last.presentation_time
                    || time
                        > last.presentation_time.saturating_add(
                            last.duration.saturating_mul(MAX_FORWARD_DECODE_FRAMES),
                        ),
                last.presentation_time,
            ),
            CursorPosition::StartedAt(origin) => (time != *origin, Duration::ZERO),
            CursorPosition::Unknown => (true, Duration::ZERO),
        };
        if seek {
            self.seek(time)?;
        }
        let mut next = self.read_interval(cancelled)?;
        if let CursorPosition::StartedAt(_) = self.position
            && next.is_none()
            && time > previous
        {
            // Some demuxers return EOS even for a keyframe seek in the audio
            // tail, including when that read resumes after cancellation. Read
            // from a known earlier position to find the final picture.
            self.seek(previous)?;
            next = self.read_interval(cancelled)?;
        }
        while let Some(frame) = next {
            self.position = CursorPosition::Frame(frame.clone());
            if frame.covers(time) || time < frame.presentation_time {
                return Ok(frame);
            }
            next = self.read_interval(cancelled)?;
        }
        match &self.position {
            CursorPosition::Frame(last) => Ok(last.clone()),
            _ => Err(MediaError::external("No video frame at the requested time")),
        }
    }
}
