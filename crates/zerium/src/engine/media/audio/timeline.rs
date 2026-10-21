use std::{
    collections::{HashMap, HashSet, VecDeque},
    ops::Range,
    sync::Arc,
};

use thiserror::Error;
use zerium_core::timeline::AudioClipId;
use zerium_core::timeline::{
    AudioTimelineSnapshot, AudioTimelineSource, EvaluatedAudioGain, Frame, FrameRate,
};

use super::super::reader::{AudioFormat, MediaError, MediaReaderRegistry};
use super::source::AudioSource;

#[derive(Clone, Copy)]
pub(crate) enum AudioCalculationPolicy {
    Report,
    Fallback,
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
    #[error("{0}")]
    Calculation(Box<zerium_core::timeline::AudioCalculationDiagnostic>),
    #[error("Missing gain for audio clip {clip:?} at sample {sample}")]
    MissingGain { clip: AudioClipId, sample: u64 },
}

struct AudioClip {
    id: AudioClipId,
    start_sample_frame: u64,
    end_sample_frame: u64,
    source: AudioSource,
}

pub(crate) struct AudioTimelineGraph {
    clips: Vec<AudioClip>,
    format: AudioFormat,
    timeline: AudioTimelineSnapshot,
    calculation_policy: AudioCalculationPolicy,
    active: Vec<usize>,
    next_clip: usize,
    previous_end: Option<u64>,
    gains: VecDeque<FrameGains>,
}

impl AudioTimelineGraph {
    pub(crate) fn new(
        timeline: AudioTimelineSnapshot,
        format: AudioFormat,
        media_readers: &MediaReaderRegistry,
        calculation_policy: AudioCalculationPolicy,
    ) -> Result<Self, AudioTimelineError> {
        let frame_rate = timeline.frame_rate();
        let mut clips = Vec::new();
        for input in timeline.sources() {
            let start_sample_frame =
                sample_boundary(input.start.get(), frame_rate, format.sample_rate);
            let end_sample_frame = sample_boundary(
                input.start.get().saturating_add(input.duration.get()),
                frame_rate,
                format.sample_rate,
            );
            if end_sample_frame <= start_sample_frame {
                continue;
            }
            let id = input.id.clone();
            let source = open_source(input, format, media_readers)?;
            clips.push(AudioClip {
                id,
                start_sample_frame,
                end_sample_frame,
                source,
            });
        }
        clips.sort_by_key(|clip| clip.start_sample_frame);
        Ok(Self {
            clips,
            format,
            timeline,
            calculation_policy,
            active: Vec::new(),
            next_clip: 0,
            previous_end: None,
            gains: VecDeque::new(),
        })
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.clips.is_empty()
    }

    /// Refresh both decoder sources and control values. Callers never need to
    /// determine whether a timeline edit requires rebuilding the mixer.
    pub(crate) fn update_timeline(
        &mut self,
        timeline: AudioTimelineSnapshot,
        media_readers: &MediaReaderRegistry,
    ) -> Result<bool, AudioTimelineError> {
        if self.timeline.same_content(&timeline) {
            return Ok(false);
        }
        if self.timeline.frame_rate() != timeline.frame_rate()
            || self.timeline.sources() != timeline.sources()
        {
            self.update_sources(&timeline, media_readers)?;
        }
        self.gains.clear();
        self.timeline = timeline;
        Ok(true)
    }

    fn update_sources(
        &mut self,
        timeline: &AudioTimelineSnapshot,
        media_readers: &MediaReaderRegistry,
    ) -> Result<(), AudioTimelineError> {
        let rate = timeline.frame_rate();
        let desired = timeline
            .sources()
            .iter()
            .filter_map(|input| {
                let start = sample_boundary(input.start.get(), rate, self.format.sample_rate);
                let end = sample_boundary(
                    input.start.get().saturating_add(input.duration.get()),
                    rate,
                    self.format.sample_rate,
                );
                (end > start).then_some((input, start..end))
            })
            .collect::<Vec<_>>();
        let previous = self
            .timeline
            .sources()
            .iter()
            .map(|input| (&input.id, input))
            .collect::<HashMap<_, _>>();
        let existing = self
            .clips
            .iter()
            .map(|clip| &clip.id)
            .collect::<HashSet<_>>();
        // Open replacements before mutating the graph, so a failed decoder
        // leaves the previous sources, cursor, and gain cache usable.
        let mut replacements = HashMap::new();
        for (input, _) in &desired {
            let reusable = existing.contains(&input.id)
                && previous
                    .get(&input.id)
                    .is_some_and(|old| old.asset == input.asset);
            if !reusable {
                replacements.insert(
                    input.id.clone(),
                    open_source(input, self.format, media_readers)?,
                );
            }
        }
        let mut previous = std::mem::take(&mut self.clips)
            .into_iter()
            .map(|clip| (clip.id, clip.source))
            .collect::<HashMap<_, _>>();
        self.clips = desired
            .into_iter()
            .map(|(input, range)| {
                let mut source = replacements
                    .remove(&input.id)
                    .unwrap_or_else(|| previous.remove(&input.id).expect("retained audio source"));
                source.update_playback(input.playback, input.preserve_pitch);
                AudioClip {
                    id: input.id.clone(),
                    source,
                    start_sample_frame: range.start,
                    end_sample_frame: range.end,
                }
            })
            .collect();
        self.clips.sort_by_key(|clip| clip.start_sample_frame);
        self.active.clear();
        self.next_clip = 0;
        self.previous_end = None;
        Ok(())
    }

    pub(crate) fn render(
        &mut self,
        block_start: u64,
        frame_count: usize,
    ) -> Result<Vec<f32>, AudioTimelineError> {
        let block_end = block_start
            .checked_add(u64::try_from(frame_count).map_err(|_| AudioTimelineError::RangeTooLarge)?)
            .ok_or(AudioTimelineError::RangeTooLarge)?;
        if frame_count == 0 {
            return Ok(Vec::new());
        }
        self.update_active(block_start, block_end);
        let channels = usize::from(self.format.channels);
        let sample_count = frame_count
            .checked_mul(channels)
            .ok_or(AudioTimelineError::RangeTooLarge)?;
        let mut mix = vec![0.; sample_count];
        let requested = self
            .active
            .iter()
            .map(|&index| self.clips[index].id.clone())
            .collect();
        let evaluated_gains = gain_segments(
            &self.timeline,
            block_start..block_end,
            self.format,
            &requested,
            &mut self.gains,
            self.calculation_policy,
        )?;
        for &index in &self.active {
            let clip = &mut self.clips[index];
            let intersection_start = block_start.max(clip.start_sample_frame);
            let intersection_end = block_end.min(clip.end_sample_frame);
            let gains =
                evaluated_gains
                    .get(&clip.id)
                    .ok_or_else(|| AudioTimelineError::MissingGain {
                        clip: clip.id.clone(),
                        sample: intersection_start,
                    })?;
            clip.mix_into(
                &mut mix,
                block_start,
                intersection_start..intersection_end,
                self.format,
                gains,
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

fn open_source(
    input: &AudioTimelineSource,
    format: AudioFormat,
    media_readers: &MediaReaderRegistry,
) -> Result<AudioSource, AudioTimelineError> {
    let decoder = media_readers
        .open_audio_decoder(&input.asset)
        .map_err(|error| AudioTimelineError::Media {
            clip: input.id.clone(),
            error,
        })?;
    Ok(AudioSource::new(
        decoder,
        input.playback,
        input.preserve_pitch,
        input.asset.duration,
        format,
    ))
}

impl AudioClip {
    fn mix_into(
        &mut self,
        mix: &mut [f32],
        block_start: u64,
        intersection: Range<u64>,
        format: AudioFormat,
        gains: &[GainSegment],
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
        let target_frame = usize::try_from(intersection.start - block_start)
            .map_err(|_| AudioTimelineError::RangeTooLarge)?;
        let mix_start = target_frame
            .checked_mul(channels)
            .ok_or(AudioTimelineError::RangeTooLarge)?;
        for segment in gains {
            let start = intersection.start.max(segment.samples.start);
            let end = intersection.end.min(segment.samples.end);
            for cursor in start..end {
                let gain = segment.value_at(cursor);
                let source_frame = (cursor - intersection.start) as usize;
                let target = mix_start + source_frame * channels;
                for channel in 0..channels {
                    mix[target + channel] += samples[source_frame * channels + channel] * gain;
                }
            }
        }
        Ok(())
    }
}

/// Adjacent authored frames are shared across render blocks. Requested IDs are
/// tracked as well, so newly active streams can join a cached frame on demand.
struct FrameGains {
    frame: Frame,
    requested: HashSet<AudioClipId>,
    values: Arc<HashMap<AudioClipId, EvaluatedAudioGain>>,
}

fn frame_gains(
    timeline: &AudioTimelineSnapshot,
    frame: Frame,
    requested: &HashSet<AudioClipId>,
    cache: &mut VecDeque<FrameGains>,
) -> Arc<HashMap<AudioClipId, EvaluatedAudioGain>> {
    let index = if let Some(index) = cache.iter().position(|entry| entry.frame == frame) {
        index
    } else {
        if cache.len() == 2 {
            cache.pop_front();
        }
        cache.push_back(FrameGains {
            frame,
            requested: HashSet::new(),
            values: Arc::default(),
        });
        cache.len() - 1
    };
    let entry = &mut cache[index];
    let missing = requested
        .difference(&entry.requested)
        .cloned()
        .collect::<HashSet<_>>();
    if !missing.is_empty() {
        Arc::make_mut(&mut entry.values).extend(timeline.gains_at_frame(frame, &missing));
        entry.requested.extend(missing);
    }
    entry.values.clone()
}

/// One frame's control ramp. Samples are interpolated while mixing decoded
/// audio, without allocating an expanded gain buffer for each clip.
struct GainSegment {
    samples: Range<u64>,
    frame_samples: Range<u64>,
    left: f32,
    right: f32,
}

impl GainSegment {
    fn value_at(&self, sample: u64) -> f32 {
        let t = (sample - self.frame_samples.start) as f64
            / (self.frame_samples.end - self.frame_samples.start) as f64;
        (f64::from(self.left) + (f64::from(self.right) - f64::from(self.left)) * t) as f32
    }
}

fn gain_segments(
    timeline: &AudioTimelineSnapshot,
    range: Range<u64>,
    format: AudioFormat,
    requested: &HashSet<AudioClipId>,
    cache: &mut VecDeque<FrameGains>,
    policy: AudioCalculationPolicy,
) -> Result<HashMap<AudioClipId, Vec<GainSegment>>, AudioTimelineError> {
    let mut values: HashMap<AudioClipId, Vec<GainSegment>> = HashMap::new();
    if requested.is_empty() {
        return Ok(values);
    }
    let rate = timeline.frame_rate();
    let mut sample = range.start;
    while sample < range.end {
        let frame = rate
            .frame_at_sample(sample, format.sample_rate)
            .expect("audio sample rate is positive");
        let next = Frame::new(
            frame
                .get()
                .checked_add(1)
                .ok_or(AudioTimelineError::RangeTooLarge)?,
        );
        let boundary = rate.sample_boundary(frame, format.sample_rate);
        let next_boundary = rate.sample_boundary(next, format.sample_rate);
        let end = range.end.min(next_boundary);
        let left = frame_gains(timeline, frame, requested, cache);
        // Only an output present on the left contributes to this interval.
        // A clip's final frame holds its last gain instead of fading to silence.
        let interpolated = if end - 1 > boundary {
            left.keys()
                .filter(|id| requested.contains(*id))
                .cloned()
                .collect()
        } else {
            HashSet::new()
        };
        let right = frame_gains(timeline, next, &interpolated, cache);
        for (id, gain) in left.iter().filter(|(id, _)| requested.contains(*id)) {
            let next_gain = right
                .get(id)
                .filter(|_| interpolated.contains(id))
                .unwrap_or(gain);
            if let AudioCalculationPolicy::Report = policy
                && let Some(error) = gain.diagnostic.as_ref().or(next_gain.diagnostic.as_ref())
            {
                return Err(AudioTimelineError::Calculation(Box::new(error.clone())));
            }
            values.entry(id.clone()).or_default().push(GainSegment {
                samples: sample..end,
                frame_samples: boundary..next_boundary,
                left: gain.value,
                right: next_gain.value,
            });
        }
        sample = end;
    }
    Ok(values)
}

pub(crate) fn sample_boundary(frame: u64, frame_rate: FrameRate, sample_rate: u32) -> u64 {
    frame_rate.sample_boundary(zerium_core::timeline::Frame::new(frame), sample_rate)
}
