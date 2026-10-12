//! Streaming source-clock conversion, shared by live playback and export.
use std::time::Duration;

use zerium_core::media::MediaPlayback;

use super::super::reader::{AudioDecoderSession, AudioFormat, MediaError};

pub(super) struct AudioSource {
    decoder: Box<dyn AudioDecoderSession>,
    playback: MediaPlayback,
    preserve_pitch: bool,
    duration: Duration,
    format: AudioFormat,
}

impl AudioSource {
    pub(super) fn new(
        decoder: Box<dyn AudioDecoderSession>,
        playback: MediaPlayback,
        preserve_pitch: bool,
        duration: Duration,
        format: AudioFormat,
    ) -> Self {
        Self {
            duration: decoder.stream_duration().min(duration),
            decoder,
            playback,
            preserve_pitch,
            format,
        }
    }

    pub(super) fn read(
        &mut self,
        start_frame: u64,
        frame_count: usize,
    ) -> Result<Vec<f32>, MediaError> {
        self.decoder
            .set_playback_rate(self.playback.speed(), self.preserve_pitch)?;
        let channels = usize::from(self.format.channels);
        let sample_count = frame_count
            .checked_mul(channels)
            .ok_or_else(|| MediaError::external("Audio block is too large"))?;
        let mut output = vec![0.; sample_count];
        let mut written = 0;
        while written < frame_count {
            let local_frame = start_frame.saturating_add(written as u64);
            let local_seconds = local_frame as f64 / f64::from(self.format.sample_rate);
            let Some(sample) = self
                .playback
                .sample(local_seconds, self.duration)
                .filter(|sample| !sample.held)
            else {
                break;
            };
            let source_seconds = sample.time.as_secs_f64();
            let until_end = ((self.duration.as_secs_f64() - source_seconds) / self.playback.speed()
                * f64::from(self.format.sample_rate))
            .ceil()
            .max(1.) as usize;
            let until_selection_end = ((self.playback.source_span()
                - local_seconds * self.playback.speed())
                / self.playback.speed()
                * f64::from(self.format.sample_rate))
            .ceil()
            .max(1.) as usize;
            let requested = (frame_count - written)
                .min(until_end)
                .min(until_selection_end);
            let decoded =
                self.decoder
                    .decode_sample_frames(source_seconds, requested, self.format)?;
            if decoded.format != self.format
                || !decoded.samples.len().is_multiple_of(channels)
                || decoded.samples.len() > requested * channels
            {
                return Err(MediaError::external("Invalid decoded audio block"));
            }
            output[written * channels..written * channels + decoded.samples.len()]
                .copy_from_slice(&decoded.samples);
            written += requested;
        }
        Ok(output)
    }
}
