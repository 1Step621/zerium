//! Bounded GStreamer H.264/AAC MP4 encoding for rendered exports.
use std::{
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
    thread,
    time::{Duration, Instant},
};

use gst::prelude::*;
use gstreamer as gst;
use gstreamer_app::{AppSrc, AppSrcCallbacks};
use thiserror::Error;
use zerium_core::media::VideoFrameRate;

use crate::engine::{
    gstreamer::{initialize, pipeline::Pipeline},
    media::AudioFormat,
};

const INPUT_QUEUE_BUFFERS: u64 = 3;
const WAIT_TIMEOUT: Duration = Duration::from_secs(60);

#[derive(Clone, Copy, Debug)]
pub(crate) struct VideoOutputSpec {
    pub(crate) width: u32,
    pub(crate) height: u32,
}

impl VideoOutputSpec {
    pub(crate) fn validate(self) -> Result<Self, EncoderError> {
        if self.width == 0 || self.height == 0 {
            return Err(EncoderError::Invalid("Video dimensions must be positive"));
        }
        if !self.width.is_multiple_of(2) || !self.height.is_multiple_of(2) {
            return Err(EncoderError::Invalid(
                "YUV420 output requires even dimensions",
            ));
        }
        // Raw-video caps and aligned plane strides use signed 32-bit dimensions.
        if self.width > i32::MAX as u32 / 4 || self.height > i32::MAX as u32 {
            return Err(EncoderError::Invalid("Video dimensions are too large"));
        }
        Ok(self)
    }
}

#[derive(Clone, Copy)]
pub(crate) struct VideoEncoderSettings {
    pub(crate) output: VideoOutputSpec,
    pub(crate) frame_rate: VideoFrameRate,
    pub(crate) gop: u32,
    pub(crate) audio: Option<AudioFormat>,
}

#[derive(Debug, Error)]
pub(crate) enum EncoderError {
    #[error("Encoding cancelled")]
    Cancelled,
    #[error("{0}")]
    Invalid(&'static str),
    #[error("Encoding failed: {0}")]
    Pipeline(String),
    #[error("Timed out waiting for the encoder")]
    Timeout,
    #[error("MP4 audio timing could not be finalized: {0}")]
    Finalize(#[from] mp4::Error),
}

impl EncoderError {
    pub(super) fn pipeline(error: impl std::fmt::Display) -> Self {
        Self::Pipeline(error.to_string())
    }
}

struct AudioInput {
    source: AppSrc,
    format: AudioFormat,
    next_sample: u64,
}

pub(crate) struct FileEncoder<'a> {
    pipeline: Pipeline,
    bus: gst::Bus,
    video: AppSrc,
    audio: Option<AudioInput>,
    frame_rate: VideoFrameRate,
    encoded_video: bool,
    cancelled: &'a AtomicBool,
    path: PathBuf,
}

impl<'a> FileEncoder<'a> {
    pub(crate) fn create(
        path: &Path,
        settings: VideoEncoderSettings,
        encoding: super::VideoEncoding,
        cancelled: &'a AtomicBool,
    ) -> Result<Self, EncoderError> {
        initialize().map_err(EncoderError::pipeline)?;
        settings.output.validate()?;
        let fps = gst::Fraction::new(
            i32::try_from(settings.frame_rate.numerator())
                .map_err(|_| EncoderError::Invalid("Frame rate is too large"))?,
            i32::try_from(settings.frame_rate.denominator())
                .map_err(|_| EncoderError::Invalid("Frame rate is too large"))?,
        );
        let encoded_video = matches!(encoding, super::VideoEncoding::H264);
        let (mut caps, video_encoder) = match encoding {
            super::VideoEncoding::H264 => (
                gst::Caps::builder("video/x-h264")
                    .field("stream-format", "byte-stream")
                    .field("alignment", "au")
                    .field("width", settings.output.width as i32)
                    .field("height", settings.output.height as i32)
                    .build(),
                None,
            ),
            super::VideoEncoding::Cpu(info) => {
                let encoder = gst::ElementFactory::make("x264enc")
                    .property_from_str("speed-preset", "medium")
                    .property_from_str("tune", "zerolatency")
                    .property_from_str("pass", "qual")
                    .property("quantizer", 18u32)
                    .property("key-int-max", settings.gop)
                    .build()
                    .map_err(EncoderError::pipeline)?;
                (
                    info.to_caps().map_err(EncoderError::pipeline)?,
                    Some(encoder),
                )
            }
        };
        caps.make_mut()
            .structure_mut(0)
            .ok_or(EncoderError::Invalid("Missing video caps"))?
            .set("framerate", fps);
        let pipeline = Pipeline::new(gst::Pipeline::new());
        let mux = gst::ElementFactory::make("mp4mux")
            .property("faststart", true)
            .build()
            .map_err(EncoderError::pipeline)?;
        if let Some(audio) = settings.audio {
            // Edit-list durations must represent each PCM sample exactly.
            mux.set_property("movie-timescale", audio.sample_rate);
        }
        let sink = gst::ElementFactory::make("filesink")
            .property(
                "location",
                path.to_str()
                    .ok_or(EncoderError::Invalid("Invalid output path"))?,
            )
            .property("sync", false)
            .build()
            .map_err(EncoderError::pipeline)?;
        pipeline
            .add_many([&mux, &sink])
            .map_err(EncoderError::pipeline)?;
        mux.link(&sink).map_err(EncoderError::pipeline)?;

        let video = app_source(&caps);
        let parse = gst::ElementFactory::make("h264parse")
            .build()
            .map_err(EncoderError::pipeline)?;
        let queue = encoded_queue()?;
        let mut elements = vec![video.clone().upcast::<gst::Element>()];
        elements.extend(video_encoder);
        elements.extend([parse, queue.clone()]);
        pipeline
            .add_many(&elements)
            .map_err(EncoderError::pipeline)?;
        gst::Element::link_many(&elements).map_err(EncoderError::pipeline)?;
        queue.link(&mux).map_err(EncoderError::pipeline)?;

        let audio = settings
            .audio
            .map(|format| {
                if format.sample_rate == 0 || format.channels == 0 || format.channels > 16 {
                    return Err(EncoderError::Invalid("Invalid audio format"));
                }
                let caps = gstreamer_audio::AudioInfo::builder(
                    gstreamer_audio::AudioFormat::F32le,
                    format.sample_rate,
                    u32::from(format.channels),
                )
                .build()
                .and_then(|info| info.to_caps())
                .map_err(EncoderError::pipeline)?;
                let source = app_source(&caps);
                let convert = gst::ElementFactory::make("audioconvert")
                    .build()
                    .map_err(EncoderError::pipeline)?;
                let encoder = gst::ElementFactory::make("avenc_aac")
                    .property("bitrate", 192_000i32)
                    .build()
                    .map_err(EncoderError::pipeline)?;
                let parse = gst::ElementFactory::make("aacparse")
                    .build()
                    .map_err(EncoderError::pipeline)?;
                let queue = encoded_queue()?;
                let elements = [source.upcast_ref(), &convert, &encoder, &parse, &queue];
                pipeline
                    .add_many(elements)
                    .map_err(EncoderError::pipeline)?;
                gst::Element::link_many(elements).map_err(EncoderError::pipeline)?;
                queue.link(&mux).map_err(EncoderError::pipeline)?;
                Ok(AudioInput {
                    source,
                    format,
                    next_sample: 0,
                })
            })
            .transpose()?;
        let bus = pipeline.bus().expect("a pipeline always has a bus");
        let result = Self {
            pipeline,
            bus,
            video,
            audio,
            frame_rate: settings.frame_rate,
            encoded_video,
            cancelled,
            path: path.to_owned(),
        };
        result.check_status()?;
        result
            .pipeline
            .set_state(gst::State::Playing)
            .map_err(EncoderError::pipeline)?;
        Ok(result)
    }

    pub(crate) fn encode_frame(&mut self, frame: super::ExportFrame) -> Result<(), EncoderError> {
        let mut buffer = gst::Buffer::from_mut_slice(frame.data);
        let start = timestamp(
            frame.index,
            self.frame_rate.denominator(),
            self.frame_rate.numerator(),
        )?;
        let end = timestamp(
            frame
                .index
                .checked_add(1)
                .ok_or(EncoderError::Invalid("Video PTS is too large"))?,
            self.frame_rate.denominator(),
            self.frame_rate.numerator(),
        )?;
        let buffer_ref = buffer
            .get_mut()
            .ok_or(EncoderError::Invalid("New video buffer is not writable"))?;
        buffer_ref.set_pts(start);
        if self.encoded_video {
            buffer_ref.set_dts(start);
            if !frame.keyframe {
                buffer_ref.set_flags(gst::BufferFlags::DELTA_UNIT);
            }
        }
        buffer_ref.set_duration(end - start);
        self.push(&self.video, buffer)
    }

    pub(crate) fn encode_audio(&mut self, samples: &[f32]) -> Result<(), EncoderError> {
        if samples.is_empty() {
            return Ok(());
        }
        let audio = self
            .audio
            .as_ref()
            .ok_or(EncoderError::Invalid("Output has no audio stream"))?;
        if !samples
            .len()
            .is_multiple_of(usize::from(audio.format.channels))
        {
            return Err(EncoderError::Invalid(
                "Audio samples must contain complete sample frames",
            ));
        }
        let frames = samples.len() / usize::from(audio.format.channels);
        let current_sample = audio.next_sample;
        let next_sample = current_sample
            .checked_add(frames as u64)
            .ok_or(EncoderError::Invalid("Audio PTS is too large"))?;
        let start = timestamp(current_sample, 1, audio.format.sample_rate)?;
        let end = timestamp(next_sample, 1, audio.format.sample_rate)?;
        let mut buffer =
            gst::Buffer::from_mut_slice(bytemuck::cast_slice::<f32, u8>(samples).to_vec());
        let buffer_ref = buffer.get_mut().expect("a new buffer is writable");
        buffer_ref.set_pts(start);
        buffer_ref.set_duration(end - start);
        self.push(&audio.source, buffer)?;
        self.audio.as_mut().expect("audio exists").next_sample = next_sample;
        Ok(())
    }

    fn push(&self, source: &AppSrc, buffer: gst::Buffer) -> Result<(), EncoderError> {
        let deadline = Instant::now() + WAIT_TIMEOUT;
        loop {
            self.check_status()?;
            if source.current_level_buffers() < INPUT_QUEUE_BUFFERS {
                break;
            }
            if Instant::now() >= deadline {
                return Err(EncoderError::Timeout);
            }
            // Wake on need-data; keep a timeout for cancellation and pipeline errors.
            thread::park_timeout(Duration::from_millis(10));
        }
        source.push_buffer(buffer).map_err(EncoderError::pipeline)?;
        self.check_status()
    }

    fn check_status(&self) -> Result<(), EncoderError> {
        if self.cancelled.load(Ordering::Relaxed) {
            return Err(EncoderError::Cancelled);
        }
        while let Some(message) = self.bus.pop() {
            if let gst::MessageView::Error(error) = message.view() {
                return Err(EncoderError::pipeline(error.error()));
            }
        }
        Ok(())
    }

    pub(crate) fn finish(self) -> Result<(), EncoderError> {
        self.check_status()?;
        self.video.end_of_stream().map_err(EncoderError::pipeline)?;
        if let Some(audio) = &self.audio {
            audio
                .source
                .end_of_stream()
                .map_err(EncoderError::pipeline)?;
        }
        let deadline = Instant::now() + WAIT_TIMEOUT;
        loop {
            if self.cancelled.load(Ordering::Relaxed) {
                return Err(EncoderError::Cancelled);
            }
            if let Some(message) = self.bus.timed_pop(gst::ClockTime::from_mseconds(10)) {
                match message.view() {
                    gst::MessageView::Eos(..) => break,
                    gst::MessageView::Error(error) => {
                        return Err(EncoderError::pipeline(error.error()));
                    }
                    _ => {}
                }
            }
            if Instant::now() >= deadline {
                return Err(EncoderError::Timeout);
            }
        }
        // Close filesink before editing the completed, unpublished file.
        self.pipeline
            .set_state(gst::State::Null)
            .map_err(EncoderError::pipeline)?;
        if let Some(audio) = &self.audio {
            if self.cancelled.load(Ordering::Relaxed) {
                return Err(EncoderError::Cancelled);
            }
            super::mp4::finalize_audio(&self.path, audio.format.sample_rate, audio.next_sample)?;
        }
        Ok(())
    }
}

fn app_source(caps: &gst::Caps) -> AppSrc {
    let producer = thread::current();
    AppSrc::builder()
        .callbacks(
            AppSrcCallbacks::builder()
                .need_data(move |_, _| producer.unpark())
                .build(),
        )
        .caps(caps)
        .format(gst::Format::Time)
        .max_buffers(INPUT_QUEUE_BUFFERS)
        .max_bytes(0)
        .block(false)
        .build()
}

fn encoded_queue() -> Result<gst::Element, EncoderError> {
    // Buffer compressed packets so AAC's frame delay cannot stall alternating
    // raw-video/audio submission. Limits apply to packets, not raw images.
    gst::ElementFactory::make("queue")
        .property("max-size-time", 1_000_000_000u64)
        .property("max-size-bytes", 8 * 1024 * 1024u32)
        .property("max-size-buffers", 0u32)
        .build()
        .map_err(EncoderError::pipeline)
}

fn timestamp(index: u64, denominator: u32, numerator: u32) -> Result<gst::ClockTime, EncoderError> {
    let nanos = u128::from(index) * u128::from(denominator) * 1_000_000_000 / u128::from(numerator);
    let nanos = u64::try_from(nanos)
        .ok()
        .filter(|nanos| *nanos < u64::MAX)
        .ok_or(EncoderError::Invalid("Presentation time is too large"))?;
    Ok(gst::ClockTime::from_nseconds(nanos))
}
