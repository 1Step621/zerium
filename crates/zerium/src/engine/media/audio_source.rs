//! Streaming source-clock conversion, shared by live playback and export.
use std::{collections::VecDeque, time::Duration};

use ffmpeg_next as ffmpeg;
use zerium_core::media::{MediaEndBehavior, MediaPlayback};

use super::reader::{AudioDecoderSession, AudioFormat, MediaError};

pub(super) struct AudioSource {
    decoder: Box<dyn AudioDecoderSession>,
    playback: MediaPlayback,
    preserve_pitch: bool,
    duration: Duration,
    decoded_duration: Duration,
    format: AudioFormat,
    stream: Option<TempoStream>,
    next_frame: Option<u64>,
    cycle: f64,
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
            decoded_duration: decoder.stream_duration().min(duration),
            duration,
            decoder,
            playback,
            preserve_pitch,
            format,
            stream: None,
            next_frame: None,
            cycle: 0.,
        }
    }

    pub(super) fn read(
        &mut self,
        start_frame: u64,
        frame_count: usize,
    ) -> Result<Vec<f32>, MediaError> {
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
            let cycle = if self.playback.end_behavior() == MediaEndBehavior::Loop {
                (self.playback.source_seconds(local_seconds) / self.duration.as_secs_f64()).floor()
            } else {
                0.
            };
            if self.next_frame != Some(local_frame) || self.cycle != cycle {
                self.stream = Some(TempoStream::new(
                    self.format,
                    self.playback,
                    self.preserve_pitch,
                    source_seconds,
                    self.decoded_duration,
                )?);
                self.cycle = cycle;
            }
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
            let stream = self
                .stream
                .as_mut()
                .expect("a source sample creates a tempo stream");
            stream.fill(&mut *self.decoder, requested)?;
            for value in &mut output[written * channels..(written + requested) * channels] {
                *value = stream.pending.pop_front().unwrap_or(0.);
            }
            written += requested;
            self.next_frame = Some(local_frame.saturating_add(requested as u64));
        }
        Ok(output)
    }
}

struct TempoStream {
    graph: ffmpeg::filter::Graph,
    format: AudioFormat,
    source_frame: u64,
    source_end: u64,
    input_pts: i64,
    discard_frames: usize,
    pending: VecDeque<f32>,
    flushed: bool,
    finished: bool,
}

fn filter_error(error: ffmpeg::Error) -> MediaError {
    MediaError::external(format!("Failed to process playback speed: {error}"))
}

impl TempoStream {
    fn new(
        format: AudioFormat,
        playback: MediaPlayback,
        preserve_pitch: bool,
        source_seconds: f64,
        duration: Duration,
    ) -> Result<Self, MediaError> {
        let mut graph = ffmpeg::filter::Graph::new();
        let layout = ffmpeg::ChannelLayout::default(i32::from(format.channels));
        let args = format!(
            "time_base=1/{}:sample_rate={}:sample_fmt=flt:channel_layout={}c",
            format.sample_rate,
            format.sample_rate,
            layout.channels()
        );
        let source = ffmpeg::filter::find("abuffer")
            .ok_or_else(|| MediaError::external("FFmpeg abuffer filter is unavailable"))?;
        let sink = ffmpeg::filter::find("abuffersink")
            .ok_or_else(|| MediaError::external("FFmpeg abuffersink filter is unavailable"))?;
        graph.add(&source, "in", &args).map_err(filter_error)?;
        graph.add(&sink, "out", "").map_err(filter_error)?;
        let spec = if preserve_pitch {
            let tempo = playback.speed().sqrt();
            format!("atempo={tempo},atempo={tempo}")
        } else {
            let rate = (f64::from(format.sample_rate) * playback.speed()).round() as u32;
            format!("asetrate={rate},aresample={}", format.sample_rate)
        };
        let spec = format!(
            "{spec},aformat=sample_fmts=flt:sample_rates={}:channel_layouts={}c",
            format.sample_rate,
            layout.channels()
        );
        graph
            .output("in", 0)
            .map_err(filter_error)?
            .input("out", 0)
            .map_err(filter_error)?
            .parse(&spec)
            .map_err(filter_error)?;
        graph.validate().map_err(filter_error)?;
        // Give the time stretcher history when seeking into the middle of a clip.
        let source_frame =
            ((source_seconds - 0.1).max(0.) * f64::from(format.sample_rate)).floor() as u64;
        let discard_frames = ((source_seconds
            - source_frame as f64 / f64::from(format.sample_rate))
            / playback.speed()
            * f64::from(format.sample_rate))
        .round() as usize;
        Ok(Self {
            graph,
            format,
            source_frame,
            source_end: (duration.as_secs_f64() * f64::from(format.sample_rate)).ceil() as u64,
            input_pts: 0,
            discard_frames,
            pending: VecDeque::new(),
            flushed: false,
            finished: false,
        })
    }

    fn fill(
        &mut self,
        decoder: &mut dyn AudioDecoderSession,
        frames: usize,
    ) -> Result<(), MediaError> {
        let channels = usize::from(self.format.channels);
        while self.pending.len() / channels < frames && !self.finished {
            let mut output = ffmpeg::frame::Audio::empty();
            match self
                .graph
                .get("out")
                .expect("output filter exists")
                .sink()
                .frame(&mut output)
            {
                Ok(()) => {
                    let skip = self.discard_frames.min(output.samples());
                    self.discard_frames -= skip;
                    if skip < output.samples() {
                        self.pending
                            .extend(output.plane::<f32>(0)[skip * channels..].iter().copied());
                    }
                }
                Err(ffmpeg::Error::Eof) => self.finished = true,
                Err(ffmpeg::Error::Other { errno }) if errno == ffmpeg::error::EAGAIN => {
                    if self.flushed {
                        return Err(MediaError::external(
                            "Audio speed filter stalled after flushing",
                        ));
                    }
                    self.feed(decoder)?;
                }
                Err(error) => return Err(filter_error(error)),
            }
        }
        Ok(())
    }

    fn feed(&mut self, decoder: &mut dyn AudioDecoderSession) -> Result<(), MediaError> {
        if self.source_frame >= self.source_end {
            return self.flush();
        }
        let requested = (self.source_end - self.source_frame).min(4096) as usize;
        let decoded = decoder.decode_sample_frames(
            self.source_frame as f64 / f64::from(self.format.sample_rate),
            requested,
            self.format,
        )?;
        let channels = usize::from(self.format.channels);
        if decoded.format != self.format || !decoded.samples.len().is_multiple_of(channels) {
            return Err(MediaError::external("Invalid decoded audio block"));
        }
        if decoded.samples.is_empty() {
            return self.flush();
        }
        let frames = (decoded.samples.len() / channels).min(requested);
        let mut input = ffmpeg::frame::Audio::new(
            ffmpeg::format::Sample::F32(ffmpeg::format::sample::Type::Packed),
            frames,
            ffmpeg::ChannelLayout::default(i32::from(self.format.channels)),
        );
        input.set_rate(self.format.sample_rate);
        input.set_pts(Some(self.input_pts));
        input
            .plane_mut::<f32>(0)
            .copy_from_slice(&decoded.samples[..frames * channels]);
        self.graph
            .get("in")
            .expect("input filter exists")
            .source()
            .add(&input)
            .map_err(filter_error)?;
        self.input_pts += frames as i64;
        self.source_frame += frames as u64;
        if frames < requested {
            self.source_frame = self.source_end;
        }
        Ok(())
    }

    fn flush(&mut self) -> Result<(), MediaError> {
        self.graph
            .get("in")
            .expect("input filter exists")
            .source()
            .flush()
            .map_err(filter_error)?;
        self.flushed = true;
        Ok(())
    }
}
