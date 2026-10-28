//! Audio streams, decoding, resampling, and playback-speed conversion.

use std::{collections::VecDeque, path::Path, time::Duration};

use gst::prelude::*;
use gstreamer as gst;
use zerium_core::media::{MediaAsset, MediaKind, MediaMetadata, MediaTarget};

use super::{DecodePipeline, MediaError, MediaReader, file_uri};
use crate::engine::initialize_gstreamer;

pub(super) const READER_ID: &str = "zerium.audio";

pub(super) struct AudioReader;

impl MediaReader for AudioReader {
    fn probe(&self, path: &Path, target: MediaTarget) -> Result<Option<MediaMetadata>, MediaError> {
        if target != MediaTarget::Audio {
            return Ok(None);
        }
        super::probe_streams(path, target)
    }

    fn open_audio_decoder(&self, asset: &MediaAsset) -> Result<Box<dyn AudioDecoder>, MediaError> {
        let MediaKind::Audio { .. } = asset.kind else {
            return Err(MediaError::invalid_input(
                "Audio reader requires audio media",
            ));
        };
        Ok(Box::new(AudioDecodeSession::open(asset.clone())?))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct AudioFormat {
    pub sample_rate: u32,
    pub channels: u16,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct DecodedAudioBlock {
    pub format: AudioFormat,
    /// Interleaved `f32` samples in channel order.
    pub samples: Vec<f32>,
}

pub(crate) trait AudioDecoder: Send {
    fn stream_duration(&self) -> Duration;

    /// Configure output speed and pitch independently; seeking uses source time.
    fn set_playback_rate(&mut self, speed: f64, pitch: f64) -> Result<(), MediaError>;

    /// Decodes `sample_frames` output frames at the configured playback rate.
    /// `start_seconds` is in source time; one frame has one sample per channel.
    /// A short or empty block indicates the end of the stream.
    fn decode_sample_frames(
        &mut self,
        start_seconds: f64,
        sample_frames: usize,
        format: AudioFormat,
    ) -> Result<DecodedAudioBlock, MediaError>;
}

struct AudioDecodeSession {
    asset: MediaAsset,
    pipeline: Option<DecodePipeline>,
    format: Option<AudioFormat>,
    speed: f64,
    pitch: f64,
    pending: VecDeque<f32>,
    next_source_seconds: Option<f64>,
    output_cursor: u64,
    finished: bool,
}

impl AudioDecodeSession {
    fn open(asset: MediaAsset) -> Result<Self, MediaError> {
        initialize_gstreamer().map_err(MediaError::external)?;
        Ok(Self {
            asset,
            pipeline: None,
            format: None,
            speed: 1.,
            pitch: 1.,
            pending: VecDeque::new(),
            next_source_seconds: None,
            output_cursor: 0,
            finished: false,
        })
    }

    fn close(&mut self) {
        self.pipeline = None;
        self.pending.clear();
        self.next_source_seconds = None;
        self.finished = false;
    }

    fn configure(&mut self, format: AudioFormat) -> Result<(), MediaError> {
        if self.format == Some(format) && self.pipeline.is_some() {
            return Ok(());
        }
        self.close();
        let pipeline = DecodePipeline::new();
        let source = gst::ElementFactory::make("uridecodebin")
            .property("uri", file_uri(&self.asset.path)?.as_str())
            .property("expose-all-streams", false)
            .property("caps", gst::Caps::builder("audio/x-raw").build())
            .build()
            .map_err(MediaError::external)?;
        let convert = gst::ElementFactory::make("audioconvert")
            .build()
            .map_err(MediaError::external)?;
        let resample = gst::ElementFactory::make("audioresample")
            .build()
            .map_err(MediaError::external)?;
        let caps = gst::Caps::builder("audio/x-raw")
            .field("format", "F32LE")
            .field("layout", "interleaved")
            .field(
                "rate",
                i32::try_from(format.sample_rate).map_err(MediaError::external)?,
            )
            .field("channels", i32::from(format.channels))
            .build();
        let mut chain = vec![convert, resample];
        if self.speed != 1. || self.pitch != 1. {
            // SoundTouch handles both tempo-only and speed/pitch changes.
            chain.push(
                gst::ElementFactory::make("pitch")
                    .property("tempo", (self.speed / self.pitch) as f32)
                    .property("rate", self.pitch as f32)
                    .build()
                    .map_err(MediaError::external)?,
            );
        }
        pipeline.sink.set_caps(Some(&caps));
        pipeline
            .pipeline
            .add(&source)
            .map_err(MediaError::external)?;
        for element in &chain {
            pipeline
                .pipeline
                .add(element)
                .map_err(MediaError::external)?;
        }
        pipeline
            .pipeline
            .add(&pipeline.sink)
            .map_err(MediaError::external)?;
        gst::Element::link_many(&chain).map_err(MediaError::external)?;
        chain
            .last()
            .unwrap()
            .link(&pipeline.sink)
            .map_err(MediaError::external)?;
        let sink_pad = chain[0].static_pad("sink").unwrap();
        source.connect_pad_added(move |_, pad| {
            if !sink_pad.is_linked()
                && pad.current_caps().is_some_and(|caps| {
                    caps.structure(0)
                        .is_some_and(|structure| structure.name() == "audio/x-raw")
                })
            {
                let _ = pad.link(&sink_pad);
            }
        });
        pipeline
            .pipeline
            .set_state(gst::State::Paused)
            .map_err(MediaError::external)?;
        pipeline
            .pull(true, None)?
            .ok_or_else(|| MediaError::external("Audio contains no samples"))?;
        self.pipeline = Some(pipeline);
        self.format = Some(format);
        Ok(())
    }

    fn seek(&mut self, source_seconds: f64, format: AudioFormat) -> Result<(), MediaError> {
        let pipeline = self.pipeline.as_ref().unwrap();
        pipeline
            .pipeline
            .set_state(gst::State::Paused)
            .map_err(MediaError::external)?;
        // A short history improves the time stretcher's output after a seek.
        let warmup = if self.speed == 1. && self.pitch == 1. {
            0.
        } else {
            0.1
        };
        let start = (source_seconds - warmup).max(0.);
        // pitch maps downstream seek times back into source time.
        let position = gst::ClockTime::from_nseconds((start / self.speed * 1e9).round() as u64);
        pipeline
            .pipeline
            .seek_simple(gst::SeekFlags::FLUSH | gst::SeekFlags::ACCURATE, position)
            .map_err(MediaError::external)?;
        self.pending.clear();
        self.output_cursor =
            (source_seconds / self.speed * f64::from(format.sample_rate)).round() as u64;
        self.finished = false;
        pipeline
            .pipeline
            .set_state(gst::State::Playing)
            .map_err(MediaError::external)?;
        Ok(())
    }

    fn append_sample(
        &mut self,
        sample: gst::Sample,
        format: AudioFormat,
    ) -> Result<(), MediaError> {
        let caps = sample
            .caps()
            .and_then(|caps| caps.structure(0))
            .ok_or_else(|| MediaError::external("Audio sample has no format"))?;
        if caps.get::<String>("format").ok().as_deref() != Some("F32LE")
            || caps.get::<String>("layout").ok().as_deref() != Some("interleaved")
            || caps.get::<i32>("rate").ok() != Some(format.sample_rate as i32)
            || caps.get::<i32>("channels").ok() != Some(i32::from(format.channels))
        {
            return Err(MediaError::external("Unexpected audio output format"));
        }
        let buffer = sample
            .buffer()
            .ok_or_else(|| MediaError::external("Audio sample has no buffer"))?;
        let mapped = buffer.map_readable().map_err(MediaError::external)?;
        let channels = usize::from(format.channels);
        if !mapped.size().is_multiple_of(channels * 4) {
            return Err(MediaError::external("Invalid audio sample buffer"));
        }
        let pts = buffer
            .pts()
            .and_then(|pts| {
                sample
                    .segment()
                    .and_then(|segment| segment.downcast_ref::<gst::ClockTime>())
                    .and_then(|segment| segment.to_stream_time(pts))
            })
            .or_else(|| buffer.pts());
        let start = pts.map_or(self.output_cursor, |pts| {
            (pts.seconds_f64() * f64::from(format.sample_rate)).round() as u64
        });
        let frames = mapped.size() / (channels * 4);
        let skip = usize::try_from(self.output_cursor.saturating_sub(start))
            .unwrap_or(usize::MAX)
            .min(frames);
        self.pending.extend(
            mapped
                .as_slice()
                .as_chunks::<4>()
                .0
                .iter()
                .skip(skip * channels)
                .map(|sample| f32::from_le_bytes(*sample)),
        );
        self.output_cursor = self.output_cursor.max(start.saturating_add(frames as u64));
        Ok(())
    }
}

impl AudioDecoder for AudioDecodeSession {
    fn stream_duration(&self) -> Duration {
        self.asset.duration
    }

    fn set_playback_rate(&mut self, speed: f64, pitch: f64) -> Result<(), MediaError> {
        if !speed.is_finite()
            || !pitch.is_finite()
            || !(0.1..=10.).contains(&pitch)
            || !(0.1..=10.).contains(&(speed / pitch))
        {
            return Err(MediaError::invalid_input("Invalid audio playback rate"));
        }
        if (self.speed, self.pitch) != (speed, pitch) {
            self.close();
            self.speed = speed;
            self.pitch = pitch;
        }
        Ok(())
    }

    fn decode_sample_frames(
        &mut self,
        start_seconds: f64,
        sample_frames: usize,
        format: AudioFormat,
    ) -> Result<DecodedAudioBlock, MediaError> {
        if !start_seconds.is_finite()
            || start_seconds < 0.
            || sample_frames == 0
            || format.sample_rate == 0
            || format.channels == 0
        {
            return Err(MediaError::invalid_input("Invalid audio sample request"));
        }
        if start_seconds >= self.asset.duration.as_secs_f64() {
            return Ok(DecodedAudioBlock {
                format,
                samples: Vec::new(),
            });
        }
        self.configure(format)?;
        let tolerance = self.speed / f64::from(format.sample_rate);
        if self
            .next_source_seconds
            .is_none_or(|next| (next - start_seconds).abs() > tolerance)
        {
            self.seek(start_seconds, format)?;
        }
        let channels = usize::from(format.channels);
        let requested = sample_frames
            .checked_mul(channels)
            .ok_or_else(|| MediaError::invalid_input("Audio block is too large"))?;
        while self.pending.len() < requested && !self.finished {
            let pipeline = self.pipeline.as_ref().unwrap();
            match pipeline.pull(false, None)? {
                Some(sample) => self.append_sample(sample, format)?,
                None => self.finished = true,
            }
        }
        let count = requested.min(self.pending.len());
        let samples = self.pending.drain(..count).collect::<Vec<_>>();
        self.next_source_seconds = Some(
            start_seconds + (count / channels) as f64 / f64::from(format.sample_rate) * self.speed,
        );
        Ok(DecodedAudioBlock { format, samples })
    }
}
