//! Media output: H.264/AAC encoding and MP4 muxing for rendered exports.
mod mp4;

use crate::engine::{
    GstreamerPipeline, initialize_gstreamer,
    input::AudioFormat,
    output::{EncodingError, EncodingSettings, VideoEncoderFrame, VideoEncoderInput},
};
use gst::prelude::*;
use gstreamer as gst;
use gstreamer_app::{AppSrc, AppSrcCallbacks};
use std::{
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
    thread,
    time::{Duration, Instant},
};
use zerium_core::media::VideoFrameRate;

const INPUT_QUEUE_BUFFERS: u64 = 3;
const WAIT_TIMEOUT: Duration = Duration::from_secs(60);

struct AudioInput {
    source: AppSrc,
    format: AudioFormat,
    next_sample: u64,
}

pub(crate) struct Mp4Writer<'a> {
    pipeline: GstreamerPipeline,
    bus: gst::Bus,
    video: AppSrc,
    audio: Option<AudioInput>,
    frame_rate: VideoFrameRate,
    encoded_video: bool,
    cancelled: &'a AtomicBool,
    path: PathBuf,
}

impl<'a> Mp4Writer<'a> {
    pub(crate) fn create(
        path: &Path,
        settings: EncodingSettings,
        encoding: VideoEncoderInput,
        cancelled: &'a AtomicBool,
    ) -> Result<Self, EncodingError> {
        initialize_gstreamer().map_err(EncodingError::backend)?;
        settings.output.validate()?;
        let fps = gst::Fraction::new(
            i32::try_from(settings.frame_rate.numerator())
                .map_err(|_| EncodingError::Invalid("Frame rate is too large"))?,
            i32::try_from(settings.frame_rate.denominator())
                .map_err(|_| EncodingError::Invalid("Frame rate is too large"))?,
        );
        let encoded_video = matches!(encoding, VideoEncoderInput::H264);
        let (mut caps, video_encoder) = match encoding {
            VideoEncoderInput::H264 => (
                gst::Caps::builder("video/x-h264")
                    .field("stream-format", "byte-stream")
                    .field("alignment", "au")
                    .field("width", settings.output.width as i32)
                    .field("height", settings.output.height as i32)
                    .build(),
                None,
            ),
            VideoEncoderInput::Nv12 => {
                let colorimetry = "bt709"
                    .parse::<gstreamer_video::VideoColorimetry>()
                    .map_err(EncodingError::backend)?;
                let info = gstreamer_video::VideoInfo::builder(
                    gstreamer_video::VideoFormat::Nv12,
                    settings.output.width,
                    settings.output.height,
                )
                .colorimetry(&colorimetry)
                .build()
                .map_err(EncodingError::backend)?;
                let encoder = gst::ElementFactory::make("x264enc")
                    .property_from_str("speed-preset", "medium")
                    .property_from_str("tune", "zerolatency")
                    .property_from_str("pass", "qual")
                    .property("quantizer", 18u32)
                    .property("key-int-max", settings.gop)
                    .build()
                    .map_err(EncodingError::backend)?;
                (
                    info.to_caps().map_err(EncodingError::backend)?,
                    Some(encoder),
                )
            }
        };
        caps.make_mut()
            .structure_mut(0)
            .ok_or(EncodingError::Invalid("Missing video caps"))?
            .set("framerate", fps);
        let pipeline = GstreamerPipeline::new(gst::Pipeline::new());
        let mux = gst::ElementFactory::make("mp4mux")
            .property("faststart", true)
            .build()
            .map_err(EncodingError::backend)?;
        if let Some(audio) = settings.audio {
            // Edit-list durations must represent each PCM sample exactly.
            mux.set_property("movie-timescale", audio.sample_rate);
        }
        let sink = gst::ElementFactory::make("filesink")
            .property(
                "location",
                path.to_str()
                    .ok_or(EncodingError::Invalid("Invalid output path"))?,
            )
            .property("sync", false)
            .build()
            .map_err(EncodingError::backend)?;
        pipeline
            .add_many([&mux, &sink])
            .map_err(EncodingError::backend)?;
        mux.link(&sink).map_err(EncodingError::backend)?;

        let video = app_source(&caps);
        let parse = gst::ElementFactory::make("h264parse")
            .build()
            .map_err(EncodingError::backend)?;
        let queue = encoded_queue()?;
        let mut elements = vec![video.clone().upcast::<gst::Element>()];
        elements.extend(video_encoder);
        elements.extend([parse, queue.clone()]);
        pipeline
            .add_many(&elements)
            .map_err(EncodingError::backend)?;
        gst::Element::link_many(&elements).map_err(EncodingError::backend)?;
        queue.link(&mux).map_err(EncodingError::backend)?;

        let audio = settings
            .audio
            .map(|format| {
                if format.sample_rate == 0 || format.channels == 0 || format.channels > 16 {
                    return Err(EncodingError::Invalid("Invalid audio format"));
                }
                let caps = gstreamer_audio::AudioInfo::builder(
                    gstreamer_audio::AudioFormat::F32le,
                    format.sample_rate,
                    u32::from(format.channels),
                )
                .build()
                .and_then(|info| info.to_caps())
                .map_err(EncodingError::backend)?;
                let source = app_source(&caps);
                let convert = gst::ElementFactory::make("audioconvert")
                    .build()
                    .map_err(EncodingError::backend)?;
                let encoder = gst::ElementFactory::make("avenc_aac")
                    .property("bitrate", 192_000i32)
                    .build()
                    .map_err(EncodingError::backend)?;
                let parse = gst::ElementFactory::make("aacparse")
                    .build()
                    .map_err(EncodingError::backend)?;
                let queue = encoded_queue()?;
                let elements = [source.upcast_ref(), &convert, &encoder, &parse, &queue];
                pipeline
                    .add_many(elements)
                    .map_err(EncodingError::backend)?;
                gst::Element::link_many(elements).map_err(EncodingError::backend)?;
                queue.link(&mux).map_err(EncodingError::backend)?;
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
            .map_err(EncodingError::backend)?;
        Ok(result)
    }

    pub(crate) fn write_video(&mut self, frame: VideoEncoderFrame) -> Result<(), EncodingError> {
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
                .ok_or(EncodingError::Invalid("Video PTS is too large"))?,
            self.frame_rate.denominator(),
            self.frame_rate.numerator(),
        )?;
        let buffer_ref = buffer
            .get_mut()
            .ok_or(EncodingError::Invalid("New video buffer is not writable"))?;
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

    pub(crate) fn write_audio(&mut self, samples: &[f32]) -> Result<(), EncodingError> {
        if samples.is_empty() {
            return Ok(());
        }
        let audio = self
            .audio
            .as_ref()
            .ok_or(EncodingError::Invalid("Output has no audio stream"))?;
        if !samples
            .len()
            .is_multiple_of(usize::from(audio.format.channels))
        {
            return Err(EncodingError::Invalid(
                "Audio samples must contain complete sample frames",
            ));
        }
        let frames = samples.len() / usize::from(audio.format.channels);
        let current_sample = audio.next_sample;
        let next_sample = current_sample
            .checked_add(frames as u64)
            .ok_or(EncodingError::Invalid("Audio PTS is too large"))?;
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

    fn push(&self, source: &AppSrc, buffer: gst::Buffer) -> Result<(), EncodingError> {
        let deadline = Instant::now() + WAIT_TIMEOUT;
        loop {
            self.check_status()?;
            if source.current_level_buffers() < INPUT_QUEUE_BUFFERS {
                break;
            }
            if Instant::now() >= deadline {
                return Err(EncodingError::Timeout);
            }
            // Wake on need-data; keep a timeout for cancellation and pipeline errors.
            thread::park_timeout(Duration::from_millis(10));
        }
        source.push_buffer(buffer).map_err(EncodingError::backend)?;
        self.check_status()
    }

    fn check_status(&self) -> Result<(), EncodingError> {
        if self.cancelled.load(Ordering::Relaxed) {
            return Err(EncodingError::Cancelled);
        }
        while let Some(message) = self.bus.pop() {
            if let gst::MessageView::Error(error) = message.view() {
                return Err(EncodingError::backend(error.error()));
            }
        }
        Ok(())
    }

    pub(crate) fn finish(self) -> Result<(), EncodingError> {
        self.check_status()?;
        self.video.end_of_stream().map_err(EncodingError::backend)?;
        if let Some(audio) = &self.audio {
            audio
                .source
                .end_of_stream()
                .map_err(EncodingError::backend)?;
        }
        let deadline = Instant::now() + WAIT_TIMEOUT;
        loop {
            if self.cancelled.load(Ordering::Relaxed) {
                return Err(EncodingError::Cancelled);
            }
            if let Some(message) = self.bus.timed_pop(gst::ClockTime::from_mseconds(10)) {
                match message.view() {
                    gst::MessageView::Eos(..) => break,
                    gst::MessageView::Error(error) => {
                        return Err(EncodingError::backend(error.error()));
                    }
                    _ => {}
                }
            }
            if Instant::now() >= deadline {
                return Err(EncodingError::Timeout);
            }
        }
        // Close filesink before editing the completed, unpublished file.
        self.pipeline
            .set_state(gst::State::Null)
            .map_err(EncodingError::backend)?;
        if let Some(audio) = &self.audio {
            if self.cancelled.load(Ordering::Relaxed) {
                return Err(EncodingError::Cancelled);
            }
            mp4::finalize_audio(&self.path, audio.format.sample_rate, audio.next_sample)?;
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

fn encoded_queue() -> Result<gst::Element, EncodingError> {
    // Buffer compressed packets so AAC's frame delay cannot stall alternating
    // raw-video/audio submission. Limits apply to packets, not raw images.
    gst::ElementFactory::make("queue")
        .property("max-size-time", 1_000_000_000u64)
        .property("max-size-bytes", 8 * 1024 * 1024u32)
        .property("max-size-buffers", 0u32)
        .build()
        .map_err(EncodingError::backend)
}

fn timestamp(
    index: u64,
    denominator: u32,
    numerator: u32,
) -> Result<gst::ClockTime, EncodingError> {
    let nanos = u128::from(index) * u128::from(denominator) * 1_000_000_000 / u128::from(numerator);
    let nanos = u64::try_from(nanos)
        .ok()
        .filter(|nanos| *nanos < u64::MAX)
        .ok_or(EncodingError::Invalid("Presentation time is too large"))?;
    Ok(gst::ClockTime::from_nseconds(nanos))
}
