//! Persistent demuxing, H.264 parsing, and software video decoding.
use super::super::{
    initialize,
    pipeline::{self, SinkPipeline},
};
use crate::engine::{
    frame::{FrameData, VideoFrame, VisualFrame},
    media::{DecodedFrame, MediaError},
};
use gst::prelude::*;
use gstreamer as gst;
use gstreamer_video as video;
use std::{
    sync::{Arc, atomic::AtomicBool},
    time::Duration,
};
use zerium_core::media::{MediaAsset, MediaKind};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum VideoInputMode {
    H264,
    Raw,
}

pub(crate) struct VideoInput {
    pipeline: SinkPipeline,
}

impl VideoInput {
    pub(crate) fn open(
        asset: &MediaAsset,
        mode: VideoInputMode,
        cancelled: &AtomicBool,
    ) -> Result<Self, MediaError> {
        initialize().map_err(MediaError::external)?;
        let gpu = mode == VideoInputMode::H264;
        let pipeline = SinkPipeline::new();
        let uri = pipeline::file_uri(&asset.path)?;
        let source = gst::ElementFactory::make("uridecodebin")
            .property("uri", uri.as_str())
            .property("expose-all-streams", false)
            .property(
                "caps",
                gst::Caps::builder(if gpu { "video/x-h264" } else { "video/x-raw" })
                    .any_features()
                    .build(),
            )
            .build()
            .map_err(MediaError::external)?;
        configure_source(&source, gpu);
        let (width, height) = match asset.kind {
            MediaKind::Video { width, height, .. } if width > 0 && height > 0 => (width, height),
            _ => return Err(MediaError::invalid_input("Invalid video dimensions")),
        };
        let mut chain: Vec<gst::Element> = Vec::new();
        let mut caps = gst::Caps::builder("video/x-raw").features([] as [&str; 0]);
        if gpu {
            chain.push(
                gst::ElementFactory::make("h264parse")
                    .build()
                    .map_err(MediaError::external)?,
            );
            caps = gst::Caps::builder("video/x-h264")
                .features([] as [&str; 0])
                .field("stream-format", "byte-stream")
                .field("alignment", "au");
        } else {
            chain.push(
                gst::ElementFactory::make("videoconvertscale")
                    .build()
                    .map_err(MediaError::external)?,
            );
            caps = caps
                .field(
                    "format",
                    gst::List::new([
                        "I420",
                        "I420_10LE",
                        "I420_12LE",
                        "Y42B",
                        "I422_10LE",
                        "I422_12LE",
                        "Y444",
                        "Y444_10LE",
                        "Y444_12LE",
                        "RGBA",
                    ]),
                )
                .field("width", width as i32)
                .field("height", height as i32)
                .field("pixel-aspect-ratio", gst::Fraction::new(1, 1));
        }
        pipeline.sink.set_caps(Some(&caps.build()));
        let alpha_sink = pipeline.sink.downgrade();
        chain.push(pipeline.sink.clone().upcast());
        pipeline
            .pipeline
            .add(&source)
            .map_err(MediaError::external)?;
        pipeline
            .pipeline
            .add_many(&chain)
            .map_err(MediaError::external)?;
        gst::Element::link_many(&chain).map_err(MediaError::external)?;
        let first = chain[0].downgrade();
        source.connect_pad_added(move |_, pad| {
            if !gpu
                && pad
                    .current_caps()
                    .and_then(|caps| video::VideoInfo::from_caps(&caps).ok())
                    .is_some_and(|info| info.has_alpha())
                && let Some(sink) = alpha_sink.upgrade()
            {
                sink.set_caps(Some(
                    &gst::Caps::builder("video/x-raw")
                        .field("format", "RGBA")
                        .field("width", width as i32)
                        .field("height", height as i32)
                        .field("pixel-aspect-ratio", gst::Fraction::new(1, 1))
                        .build(),
                ));
            }
            if !pad.current_caps().is_some_and(|caps| {
                caps.structure(0).is_some_and(|s| {
                    if gpu {
                        s.name() == "video/x-h264"
                    } else {
                        s.name().starts_with("video/")
                    }
                })
            }) {
                return;
            }
            if let Some(first) = first.upgrade()
                && let Some(sink_pad) = first.static_pad("sink")
                && !sink_pad.is_linked()
            {
                let _ = pad.link(&sink_pad);
            }
        });
        pipeline
            .pipeline
            .set_state(gst::State::Paused)
            .map_err(MediaError::external)?;
        let sample = pipeline
            .pull(true, Some(cancelled))?
            .ok_or_else(|| MediaError::external("Video contains no frames"))?;
        if !gpu {
            Self::frame(sample)?;
        }
        pipeline
            .pipeline
            .set_state(gst::State::Playing)
            .map_err(MediaError::external)?;
        Ok(Self { pipeline })
    }

    pub(crate) fn read_sample(
        &self,
        cancelled: &AtomicBool,
    ) -> Result<Option<gst::Sample>, MediaError> {
        self.pipeline.pull(false, Some(cancelled))
    }

    pub(crate) fn seek(&self, time: Duration) -> Result<(), MediaError> {
        self.pipeline
            .sink
            .seek_simple(
                gst::SeekFlags::FLUSH | gst::SeekFlags::KEY_UNIT | gst::SeekFlags::SNAP_BEFORE,
                gst::ClockTime::from_nseconds(
                    u64::try_from(time.as_nanos()).map_err(MediaError::external)?,
                ),
            )
            .map_err(MediaError::external)
    }

    fn frame(sample: gst::Sample) -> Result<DecodedFrame, MediaError> {
        let caps = sample
            .caps()
            .ok_or_else(|| MediaError::external("Video frame has no format"))?;
        let info = video::VideoInfo::from_caps(caps).map_err(MediaError::external)?;
        let buffer = sample
            .buffer_owned()
            .ok_or_else(|| MediaError::external("Video frame has no buffer"))?;
        let pts = buffer
            .pts()
            .and_then(|pts| {
                sample
                    .segment()
                    .and_then(|segment| segment.downcast_ref::<gst::ClockTime>())
                    .and_then(|segment| segment.to_stream_time(pts))
            })
            .or_else(|| buffer.pts());
        let presentation_time = Duration::from_nanos(pts.map_or(0, |pts| pts.nseconds()));
        let duration = buffer
            .duration()
            .map(|duration| Duration::from_nanos(duration.nseconds()))
            .unwrap_or_else(|| {
                let fps = info.fps();
                if fps.numer() > 0 {
                    Duration::from_secs_f64(f64::from(fps.denom()) / f64::from(fps.numer()))
                } else {
                    Duration::from_secs_f64(1. / 30.)
                }
            })
            .max(Duration::from_nanos(1));
        let frame = VisualFrame {
            width: info.width(),
            height: info.height(),
            data: FrameData::Video(Arc::new(VideoFrame::from_cpu(info, buffer))),
        };
        Ok(DecodedFrame {
            presentation_time,
            duration,
            frame,
        })
    }

    pub(crate) fn read_frame(
        &self,
        cancelled: &AtomicBool,
    ) -> Result<Option<DecodedFrame>, MediaError> {
        loop {
            let Some(sample) = self.read_sample(cancelled)? else {
                return Ok(None);
            };
            // Accurate seeks can leave a one-nanosecond boundary fragment of
            // the preceding frame. Do not cache it in place of the next frame.
            if sample.buffer().is_some_and(|buffer| {
                buffer
                    .duration()
                    .is_some_and(|duration| duration.nseconds() <= 1)
            }) {
                continue;
            }
            return Self::frame(sample).map(Some);
        }
    }
}

/// Keep demuxing and parsing in GStreamer; only the CPU route opens video decoders.
fn configure_source(source: &gst::Element, gpu: bool) {
    let signal = gst::glib::subclass::SignalId::lookup("autoplug-select", source.type_())
        .expect("uridecodebin provides autoplug-select");
    let result = gst::glib::EnumClass::with_type(signal.query().return_type().type_())
        .expect("autoplug-select returns an enum");
    source.connect("autoplug-select", false, move |values| {
        let factory = values[3]
            .get::<gst::ElementFactory>()
            .expect("decoder factory");
        let skip = factory.metadata("klass").is_some_and(|klass| {
            klass.contains("Decoder/Video") && (gpu || klass.contains("Hardware"))
        });
        result.to_value_by_nick(if skip { "skip" } else { "try" })
    });
}
