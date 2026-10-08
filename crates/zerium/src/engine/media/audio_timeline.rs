use std::{
    collections::HashMap,
    ops::Range,
    sync::{
        Arc,
        atomic::{AtomicU32, Ordering},
    },
};

use thiserror::Error;

use zerium_core::timeline::{FrameRate, ItemId, TimelineItem, TimelineTime};

use super::{
    audio_source::AudioSource,
    reader::{AudioFormat, MediaError, MediaReaderRegistry},
};

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct AudioClipId {
    pub(crate) item_id: ItemId,
    pub(crate) input_id: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AudioGainEvaluation {
    Live,
    TimelineAnimation,
}

#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub(crate) enum AudioTimelineError {
    #[error(
        "Failed to process audio input '{}' (item {:?}): {error}",
        clip.input_id,
        clip.item_id
    )]
    Media {
        clip: AudioClipId,
        #[source]
        error: MediaError,
    },
    #[error("Audio render range is too large")]
    RangeTooLarge,
}

struct AudioClip {
    id: AudioClipId,
    item: TimelineItem,
    start_sample_frame: u64,
    end_sample_frame: u64,
    live_gain: Arc<AtomicU32>,
    source: AudioSource,
}

pub(crate) struct AudioTimelineGraph {
    clips: Vec<AudioClip>,
    format: AudioFormat,
    frame_rate: FrameRate,
    gain_evaluation: AudioGainEvaluation,
    active: Vec<usize>,
    next_clip: usize,
    previous_end: Option<u64>,
}

impl AudioTimelineGraph {
    pub(crate) fn new(
        items: &[TimelineItem],
        cache: &zerium_core::media::MediaMetadataCache,
        frame_rate: FrameRate,
        format: AudioFormat,
        media_readers: &MediaReaderRegistry,
        gain_evaluation: AudioGainEvaluation,
    ) -> Result<Self, AudioTimelineError> {
        let mut clips = Vec::new();
        for item in items {
            let Some(schema) = item.schema() else {
                continue;
            };
            let start_sample_frame =
                sample_boundary(item.start.get(), frame_rate, format.sample_rate);
            let end_sample_frame = sample_boundary(
                item.start.get().saturating_add(item.duration.get()),
                frame_rate,
                format.sample_rate,
            );
            for input in schema.audio() {
                let Some((asset, playback, preserve_pitch)) = item.audio_input(input.id(), cache)
                else {
                    continue;
                };
                let id = AudioClipId {
                    item_id: item.id,
                    input_id: input.id().to_owned(),
                };
                let live_gain = Arc::new(AtomicU32::new(
                    item.audio_gain(input.id())
                        .expect("validated audio input")
                        .to_bits(),
                ));
                let decoder = media_readers.open_audio_decoder(&asset).map_err(|error| {
                    AudioTimelineError::Media {
                        clip: id.clone(),
                        error,
                    }
                })?;
                let source =
                    AudioSource::new(decoder, playback, preserve_pitch, asset.duration, format);
                clips.push(AudioClip {
                    id,
                    item: item.clone(),
                    start_sample_frame,
                    end_sample_frame,
                    live_gain,
                    source,
                });
            }
        }
        clips.sort_by_key(|clip| clip.start_sample_frame);
        Ok(Self {
            clips,
            format,
            frame_rate,
            gain_evaluation,
            active: Vec::new(),
            next_clip: 0,
            previous_end: None,
        })
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.clips.is_empty()
    }

    pub(crate) fn live_gains(&self) -> HashMap<AudioClipId, Arc<AtomicU32>> {
        self.clips
            .iter()
            .map(|clip| (clip.id.clone(), clip.live_gain.clone()))
            .collect()
    }

    pub(crate) fn render(
        &mut self,
        block_start: u64,
        frame_count: usize,
    ) -> Result<Vec<f32>, AudioTimelineError> {
        let block_end = block_start
            .checked_add(u64::try_from(frame_count).map_err(|_| AudioTimelineError::RangeTooLarge)?)
            .ok_or(AudioTimelineError::RangeTooLarge)?;
        self.update_active(block_start, block_end);
        let channels = usize::from(self.format.channels);
        let sample_count = frame_count
            .checked_mul(channels)
            .ok_or(AudioTimelineError::RangeTooLarge)?;
        let mut mix = vec![0.; sample_count];
        for &index in &self.active {
            let clip = &mut self.clips[index];
            let intersection_start = block_start.max(clip.start_sample_frame);
            let intersection_end = block_end.min(clip.end_sample_frame);
            clip.mix_into(
                &mut mix,
                block_start,
                intersection_start..intersection_end,
                self.format,
                self.frame_rate,
                self.gain_evaluation,
            )?;
        }
        for sample in &mut mix {
            *sample = sample.clamp(-1., 1.);
        }
        self.previous_end = Some(block_end);
        Ok(mix)
    }

    fn update_active(&mut self, block_start: u64, block_end: u64) {
        if self.previous_end != Some(block_start) {
            self.active.clear();
            self.next_clip = 0;
        }

        self.active
            .retain(|index| self.clips[*index].end_sample_frame > block_start);
        while self.next_clip < self.clips.len()
            && self.clips[self.next_clip].start_sample_frame < block_end
        {
            if self.clips[self.next_clip].end_sample_frame > block_start {
                self.active.push(self.next_clip);
            }
            self.next_clip += 1;
        }
    }
}

impl AudioClip {
    fn mix_into(
        &mut self,
        mix: &mut [f32],
        block_start: u64,
        intersection: Range<u64>,
        format: AudioFormat,
        frame_rate: FrameRate,
        gain_evaluation: AudioGainEvaluation,
    ) -> Result<(), AudioTimelineError> {
        let channels = usize::from(format.channels);
        let frame_count = usize::try_from(intersection.end.saturating_sub(intersection.start))
            .map_err(|_| AudioTimelineError::RangeTooLarge)?;
        let source_offset = intersection.start.saturating_sub(self.start_sample_frame);
        let samples = self
            .source
            .read(source_offset, frame_count)
            .map_err(|error| AudioTimelineError::Media {
                clip: self.id.clone(),
                error,
            })?;
        let target_frame = usize::try_from(intersection.start.saturating_sub(block_start))
            .map_err(|_| AudioTimelineError::RangeTooLarge)?;
        let gain_start = self.gain_at(intersection.start, format, frame_rate, gain_evaluation);
        let gain_end = self.gain_at(intersection.end, format, frame_rate, gain_evaluation);
        let mix_start = target_frame
            .checked_mul(channels)
            .ok_or(AudioTimelineError::RangeTooLarge)?;
        for frame in 0..frame_count {
            let progress = frame as f32 / frame_count.max(1) as f32;
            let gain = gain_start + (gain_end - gain_start) * progress;
            for channel in 0..channels {
                mix[mix_start + frame * channels + channel] +=
                    samples[frame * channels + channel] * gain;
            }
        }
        Ok(())
    }

    fn gain_at(
        &self,
        sample_frame: u64,
        format: AudioFormat,
        frame_rate: FrameRate,
        evaluation: AudioGainEvaluation,
    ) -> f32 {
        match evaluation {
            AudioGainEvaluation::Live => {
                f32::from_bits(self.live_gain.load(Ordering::Relaxed)).max(0.)
            }
            AudioGainEvaluation::TimelineAnimation => {
                let seconds = sample_frame as f64 / f64::from(format.sample_rate);
                let time = TimelineTime::from_frames(seconds * frame_rate.frames_per_second());
                self.item
                    .evaluated_at_time(time)
                    .audio_gain(&self.id.input_id)
                    .expect("validated audio input")
            }
        }
    }
}

pub(crate) fn sample_boundary(frame: u64, frame_rate: FrameRate, sample_rate: u32) -> u64 {
    let samples = u128::from(frame)
        .saturating_mul(u128::from(sample_rate))
        .saturating_mul(u128::from(frame_rate.denominator()))
        / u128::from(frame_rate.numerator());
    u64::try_from(samples).unwrap_or(u64::MAX)
}
