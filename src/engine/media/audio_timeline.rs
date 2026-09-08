use std::{
    collections::HashMap,
    error::Error,
    fmt,
    sync::{
        Arc,
        atomic::{AtomicU32, Ordering},
    },
};

use crate::domain::timeline::{FrameRate, ItemId, TimelineItem, TimelineTime};

use super::reader::{AudioDecoderSession, AudioFormat, MediaError, MediaReaderRegistry};

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

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum AudioTimelineError {
    Media {
        clip: AudioClipId,
        error: MediaError,
    },
    InvalidDecoderOutput {
        clip: AudioClipId,
    },
    RangeTooLarge,
}

impl fmt::Display for AudioTimelineError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Media { clip, error } => write!(
                formatter,
                "音声入力'{}'（item {:?}）を処理できません: {error}",
                clip.input_id, clip.item_id
            ),
            Self::InvalidDecoderOutput { clip } => write!(
                formatter,
                "音声入力'{}'（item {:?}）が不正な形式を返しました",
                clip.input_id, clip.item_id
            ),
            Self::RangeTooLarge => formatter.write_str("音声レンダー範囲が大きすぎます"),
        }
    }
}

impl Error for AudioTimelineError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Media { error, .. } => Some(error),
            Self::InvalidDecoderOutput { .. } | Self::RangeTooLarge => None,
        }
    }
}

struct AudioClip {
    id: AudioClipId,
    item: TimelineItem,
    start_sample_frame: u64,
    end_sample_frame: u64,
    source_sample_frames: u64,
    live_gain: Arc<AtomicU32>,
    decoder: Box<dyn AudioDecoderSession>,
}

pub(crate) struct AudioTimelineGraph {
    clips: Vec<AudioClip>,
    format: AudioFormat,
    frame_rate: FrameRate,
    gain_evaluation: AudioGainEvaluation,
    active: Vec<usize>,
    next_clip: usize,
    previous_end: Option<u64>,
    live_gains: HashMap<ItemId, Arc<AtomicU32>>,
}

impl AudioTimelineGraph {
    pub(crate) fn new(
        items: Vec<TimelineItem>,
        frame_rate: FrameRate,
        format: AudioFormat,
        media_readers: &MediaReaderRegistry,
        gain_evaluation: AudioGainEvaluation,
    ) -> Result<Self, AudioTimelineError> {
        let mut clips = Vec::new();
        let mut live_gains = HashMap::new();
        for item in items {
            let Some(schema) = item.schema() else {
                continue;
            };
            let Some(audio) = schema.audio().cloned() else {
                continue;
            };
            let start_sample_frame =
                sample_boundary(item.start.get(), frame_rate, format.sample_rate);
            let end_sample_frame = sample_boundary(
                item.start.get().saturating_add(item.duration.get()),
                frame_rate,
                format.sample_rate,
            );
            let live_gain = live_gains
                .entry(item.id)
                .or_insert_with(|| Arc::new(AtomicU32::new(item.audio_gain().to_bits())))
                .clone();
            for (input_id, asset) in &item.assets {
                if !audio.consumes(input_id) || !asset.kind.has_audio() {
                    continue;
                }
                let decoder = media_readers.open_audio_decoder(asset).map_err(|error| {
                    AudioTimelineError::Media {
                        clip: AudioClipId {
                            item_id: item.id,
                            input_id: input_id.clone(),
                        },
                        error,
                    }
                })?;
                let source_sample_frames = seconds_to_sample_frame(
                    decoder.stream_duration().as_secs_f64(),
                    format.sample_rate,
                )
                .max(1);
                clips.push(AudioClip {
                    id: AudioClipId {
                        item_id: item.id,
                        input_id: input_id.clone(),
                    },
                    item: item.clone(),
                    start_sample_frame,
                    end_sample_frame,
                    source_sample_frames,
                    live_gain: live_gain.clone(),
                    decoder,
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
            live_gains,
        })
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.clips.is_empty()
    }

    pub(crate) fn live_gains(&self) -> HashMap<ItemId, Arc<AtomicU32>> {
        self.live_gains.clone()
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
        let active = self.active.clone();
        for index in active {
            let clip = &mut self.clips[index];
            let intersection_start = block_start.max(clip.start_sample_frame);
            let intersection_end = block_end.min(clip.end_sample_frame);
            mix_clip(
                &mut mix,
                block_start,
                intersection_start,
                intersection_end,
                clip,
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
            while self.next_clip < self.clips.len()
                && self.clips[self.next_clip].start_sample_frame < block_end
            {
                if self.clips[self.next_clip].end_sample_frame > block_start {
                    self.active.push(self.next_clip);
                }
                self.next_clip += 1;
            }
            return;
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

#[allow(clippy::too_many_arguments)]
fn mix_clip(
    mix: &mut [f32],
    block_start: u64,
    intersection_start: u64,
    intersection_end: u64,
    clip: &mut AudioClip,
    format: AudioFormat,
    frame_rate: FrameRate,
    gain_evaluation: AudioGainEvaluation,
) -> Result<(), AudioTimelineError> {
    let channels = usize::from(format.channels);
    let mut remaining = usize::try_from(intersection_end.saturating_sub(intersection_start))
        .map_err(|_| AudioTimelineError::RangeTooLarge)?;
    let timeline_source_offset = intersection_start.saturating_sub(clip.start_sample_frame);
    let mut source_frame = timeline_source_offset % clip.source_sample_frames;
    let mut target_frame = usize::try_from(intersection_start.saturating_sub(block_start))
        .map_err(|_| AudioTimelineError::RangeTooLarge)?;

    while remaining > 0 {
        let until_loop_end =
            usize::try_from(clip.source_sample_frames.saturating_sub(source_frame))
                .unwrap_or(usize::MAX);
        let requested = remaining.min(until_loop_end.max(1));
        let source_seconds = source_frame as f64 / f64::from(format.sample_rate);
        let decoded = clip
            .decoder
            .decode_sample_frames(source_seconds, requested, format)
            .map_err(|error| AudioTimelineError::Media {
                clip: clip.id.clone(),
                error,
            })?;
        if decoded.format != format || !decoded.samples.len().is_multiple_of(channels) {
            return Err(AudioTimelineError::InvalidDecoderOutput {
                clip: clip.id.clone(),
            });
        }
        let decoded_frames = (decoded.samples.len() / channels).min(requested);
        if decoded_frames == 0 {
            return Err(AudioTimelineError::InvalidDecoderOutput {
                clip: clip.id.clone(),
            });
        }
        let timeline_frame = block_start.saturating_add(target_frame as u64);
        let gain_start = gain_at(clip, timeline_frame, format, frame_rate, gain_evaluation);
        let gain_end = gain_at(
            clip,
            timeline_frame.saturating_add(decoded_frames as u64),
            format,
            frame_rate,
            gain_evaluation,
        );
        let mix_start = target_frame
            .checked_mul(channels)
            .ok_or(AudioTimelineError::RangeTooLarge)?;
        for frame in 0..decoded_frames {
            let progress = frame as f32 / decoded_frames.max(1) as f32;
            let gain = gain_start + (gain_end - gain_start) * progress;
            for channel in 0..channels {
                mix[mix_start + frame * channels + channel] +=
                    decoded.samples[frame * channels + channel] * gain;
            }
        }
        remaining -= decoded_frames;
        target_frame += decoded_frames;
        source_frame = source_frame.saturating_add(decoded_frames as u64);
        if decoded_frames < requested || source_frame >= clip.source_sample_frames {
            source_frame = 0;
        }
    }
    Ok(())
}

fn gain_at(
    clip: &AudioClip,
    sample_frame: u64,
    format: AudioFormat,
    frame_rate: FrameRate,
    evaluation: AudioGainEvaluation,
) -> f32 {
    match evaluation {
        AudioGainEvaluation::Live => f32::from_bits(clip.live_gain.load(Ordering::Relaxed)).max(0.),
        AudioGainEvaluation::TimelineAnimation => {
            let seconds = sample_frame as f64 / f64::from(format.sample_rate);
            let time = TimelineTime::from_frames(seconds * frame_rate.frames_per_second());
            clip.item.evaluated_at_time(time).audio_gain()
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

fn seconds_to_sample_frame(seconds: f64, sample_rate: u32) -> u64 {
    (seconds.max(0.) * f64::from(sample_rate))
        .round()
        .clamp(0., u64::MAX as f64) as u64
}
