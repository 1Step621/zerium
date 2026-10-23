//! Presentation-interval selection, independent of the decoding implementation.
use super::backend::DecodeBackend;
use crate::engine::media::{DecodedFrame, MediaError};
use std::{
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};

// Consume nearby forward requests sequentially; distant requests use keyframe seeks.
const MAX_FORWARD_DECODE_FRAMES: u32 = 10;

pub(super) struct PlaybackCursor {
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
    pub(super) fn new(backend: DecodeBackend, duration: Duration) -> Self {
        Self {
            backend,
            duration,
            position: CursorPosition::StartedAt(Duration::ZERO),
            lookahead: None,
        }
    }

    pub(super) fn is_gpu(&self) -> bool {
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

    pub(super) fn frame_at(
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
        if next.is_none()
            && matches!(self.position, CursorPosition::StartedAt(_))
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
