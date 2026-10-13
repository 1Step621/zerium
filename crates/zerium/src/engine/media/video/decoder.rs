//! Source-frame selection, persistent decoder state, and GPU-to-CPU fallback.
use super::gpu_decoder::GpuVideoDecoder;
use crate::engine::{
    gpu::GpuContext,
    gstreamer::input::{VideoInput, VideoInputMode},
    media::{DecodedFrame, MediaError, VideoDecoder},
};
use std::{
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};
use zerium_core::media::MediaAsset;

pub(crate) struct VideoDecodeSession {
    asset: MediaAsset,
    backend: Option<DecodeBackend>,
    last: Option<DecodedFrame>,
    lookahead: Option<DecodedFrame>,
    gpu: Option<GpuContext>,
}

enum DecodeBackend {
    Gpu {
        input: VideoInput,
        decoder: GpuVideoDecoder,
    },
    Cpu(VideoInput),
}

impl VideoDecodeSession {
    pub(crate) fn open(asset: MediaAsset, gpu: Option<&GpuContext>) -> Result<Self, MediaError> {
        Ok(Self {
            asset,
            backend: None,
            last: None,
            lookahead: None,
            gpu: gpu.cloned(),
        })
    }

    fn close(&mut self) {
        self.backend = None;
        self.last = None;
        self.lookahead = None;
    }

    fn configure(&mut self, cancelled: &AtomicBool) -> Result<(), MediaError> {
        if self.backend.is_some() {
            return Ok(());
        }
        let backend = match &self.gpu {
            Some(gpu) => {
                let decoder = GpuVideoDecoder::new(gpu)?;
                let input = VideoInput::open(&self.asset, VideoInputMode::H264, cancelled)?;
                DecodeBackend::Gpu { input, decoder }
            }
            None => DecodeBackend::Cpu(VideoInput::open(
                &self.asset,
                VideoInputMode::Raw,
                cancelled,
            )?),
        };
        self.backend = Some(backend);
        Ok(())
    }

    fn next(&mut self, cancelled: &AtomicBool) -> Result<Option<DecodedFrame>, MediaError> {
        match self.backend.as_mut().expect("configured video decoder") {
            DecodeBackend::Gpu { input, decoder } => decoder.next(input, cancelled),
            DecodeBackend::Cpu(input) => input.read_frame(cancelled),
        }
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
                let Some(frame) = self.next(cancelled)? else {
                    return Ok(None);
                };
                frame
            }
        };
        loop {
            let Some(next) = self.next(cancelled)? else {
                // A container can keep playing audio after video ends. The
                // final picture covers that tail, including requests after a seek.
                current.duration = current.duration.max(
                    self.asset
                        .duration
                        .saturating_sub(current.presentation_time),
                );
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

    fn seek(&mut self, time: Duration) -> Result<(), MediaError> {
        self.last = None;
        self.lookahead = None;
        // Preserve full presentation intervals, including an audio-only tail.
        match self.backend.as_mut().expect("configured video decoder") {
            DecodeBackend::Gpu { input, decoder } => {
                decoder.seek()?;
                input.seek(time)
            }
            DecodeBackend::Cpu(input) => input.seek(time),
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
            Err(error) if self.gpu.is_some() => {
                eprintln!("GPU video decoding unavailable; using CPU decoding: {error}");
                self.close();
                self.gpu = None;
                self.decode_frame(time, cancelled)
            }
            result => result,
        }
    }
}

impl VideoDecodeSession {
    fn decode_frame(
        &mut self,
        time: Duration,
        cancelled: &AtomicBool,
    ) -> Result<DecodedFrame, MediaError> {
        if cancelled.load(Ordering::Relaxed) {
            return Err(MediaError::Cancelled);
        }
        self.configure(cancelled)?;
        if let Some(last) = &self.last
            && last.covers(time)
        {
            return Ok(last.clone());
        }
        // Nearby forward requests consume the existing stream. Backward and
        // distant requests decode from the preceding keyframe to the target.
        let previous = self
            .last
            .as_ref()
            .map_or(Duration::ZERO, |last| last.presentation_time);
        let seek = self.last.as_ref().map_or(!time.is_zero(), |last| {
            time < last.presentation_time
                || time
                    > last
                        .presentation_time
                        .saturating_add(last.duration.saturating_mul(10))
        });
        if seek {
            self.seek(time)?;
        }
        let mut next = self.read_interval(cancelled)?;
        if next.is_none() && seek && time > previous {
            // Some demuxers return EOS even for a keyframe seek in the audio
            // tail. Read from a known earlier position to find the final picture.
            self.seek(previous)?;
            next = self.read_interval(cancelled)?;
        }
        while let Some(frame) = next {
            self.last = Some(frame.clone());
            if time.saturating_add(Duration::from_nanos(1))
                < frame.presentation_time.saturating_add(frame.duration)
            {
                return Ok(frame);
            }
            next = self.read_interval(cancelled)?;
        }
        self.last
            .clone()
            .ok_or_else(|| MediaError::external("No video frame at the requested time"))
    }
}
