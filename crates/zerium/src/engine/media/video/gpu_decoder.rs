//! H.264 bitstream decoding into textures on the consumer's GPU device.
use crate::engine::gpu::GpuContext;
use std::{
    collections::BTreeMap,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::Duration,
};

use gpu_video::{OutputFrame, VideoDeviceExt, VideoTexture, WgpuTexturesDecoderH264};
use gstreamer as gst;
use gstreamer_video as video;

use crate::engine::frame::{VideoFrame, yuv_format};
use crate::engine::{
    frame::{FrameData, VisualFrame, YuvMatrix},
    gstreamer::input::VideoInput,
    media::{DecodedFrame, MediaError},
};

pub(super) struct GpuVideoDecoder {
    device: wgpu::Device,
    decoder: WgpuTexturesDecoderH264,
    frames: mpsc::Receiver<OutputFrame<VideoTexture>>,
    pending: BTreeMap<u64, SampleMetadata>,
    finished: bool,
}

struct SampleMetadata {
    duration: Duration,
    color: Option<video::VideoColorimetry>,
    chroma_site: video::VideoChromaSite,
}

impl GpuVideoDecoder {
    pub(super) fn new(gpu: &GpuContext) -> Result<Self, MediaError> {
        let device = &gpu.device;
        let queue = &gpu.queue;
        let video = device.video().map_err(MediaError::external)?;
        if video.decode_capabilities().h264.is_none() {
            return Err(MediaError::external("GPU H.264 decoding is unavailable"));
        }
        let (sender, frames) = mpsc::channel();
        let decoder = video
            .create_wgpu_textures_decoder_h264(
                queue,
                gpu_video::parameters::DecoderParameters::default(),
                move |frame| {
                    // The library shares its callback thread with other GPU consumers.
                    let _ = sender.send(frame);
                },
            )
            .map_err(MediaError::external)?;
        Ok(Self {
            device: device.clone(),
            decoder,
            frames,
            pending: BTreeMap::new(),
            finished: false,
        })
    }

    pub(super) fn next(
        &mut self,
        input: &VideoInput,
        cancelled: &AtomicBool,
    ) -> Result<Option<DecodedFrame>, MediaError> {
        loop {
            if cancelled.load(Ordering::Relaxed) {
                return Err(MediaError::Cancelled);
            }
            match self.frames.try_recv() {
                Ok(frame) => return self.frame(frame).map(Some),
                Err(mpsc::TryRecvError::Disconnected) => {
                    return Err(MediaError::external("GPU video decoder stopped"));
                }
                Err(mpsc::TryRecvError::Empty) => {}
            }
            if self.finished {
                return Ok(None);
            }
            let Some(sample) = input.read_sample(cancelled)? else {
                // Publish pictures retained for presentation-order reordering.
                self.decoder.flush().map_err(MediaError::external)?;
                self.finished = true;
                continue;
            };
            let buffer = sample
                .buffer()
                .ok_or_else(|| MediaError::external("Encoded video has no buffer"))?;
            let pts = buffer
                .pts()
                .and_then(|pts| {
                    sample
                        .segment()
                        .and_then(|segment| segment.downcast_ref::<gst::ClockTime>())
                        .and_then(|segment| segment.to_stream_time(pts))
                })
                .or_else(|| buffer.pts())
                .ok_or_else(|| MediaError::external("Encoded video has no presentation timestamp"))?
                .nseconds();
            let duration = buffer
                .duration()
                .map(|duration| Duration::from_nanos(duration.nseconds()))
                .unwrap_or(Duration::from_secs_f64(1. / 30.))
                .max(Duration::from_nanos(1));
            let caps = sample.caps().and_then(|caps| caps.structure(0));
            let color = caps
                .and_then(|caps| caps.get::<String>("colorimetry").ok())
                .map(|color| color.parse::<video::VideoColorimetry>())
                .transpose()
                .map_err(MediaError::external)?;
            let chroma_site = caps
                .and_then(|caps| caps.get::<String>("chroma-site").ok())
                .map(|site| site.parse::<video::VideoChromaSite>())
                .transpose()
                .map_err(MediaError::external)?
                .unwrap_or(video::VideoChromaSite::H_COSITED);
            self.pending.insert(
                pts,
                SampleMetadata {
                    duration,
                    color,
                    chroma_site,
                },
            );
            if self.pending.len() > 64 {
                return Err(MediaError::external(
                    "GPU decoder is not producing pictures",
                ));
            }
            let bytes = buffer.map_readable().map_err(MediaError::external)?;
            self.decoder
                .decode(gpu_video::EncodedInputChunk {
                    data: bytes.as_slice(),
                    pts: Some(pts),
                })
                .map_err(MediaError::external)?;
            // h264parse supplies complete access units, including parameter sets after seeks.
            self.decoder
                .process_event(gpu_video::DecoderEvent::SignalFrameEnd)
                .map_err(MediaError::external)?;
        }
    }

    pub(super) fn seek(&mut self) -> Result<(), MediaError> {
        // Finish and discard the old presentation sequence before reading a new keyframe.
        // Keep the decoder's video session and surfaces allocated across seeks.
        self.decoder.flush().map_err(MediaError::external)?;
        while self.frames.try_recv().is_ok() {}
        self.pending.clear();
        self.finished = false;
        self.decoder
            .process_event(gpu_video::DecoderEvent::SignalDataLoss)
            .map_err(MediaError::external)
    }

    fn frame(&mut self, output: OutputFrame<VideoTexture>) -> Result<DecodedFrame, MediaError> {
        let pts = output.metadata.pts.ok_or_else(|| {
            MediaError::external("Decoded GPU frame has no presentation timestamp")
        })?;
        let sample = self
            .pending
            .remove(&pts)
            .ok_or_else(|| MediaError::external("GPU frame has no input timing metadata"))?;
        // The decoder delivers presentation order; discard metadata for dropped pictures.
        self.pending.retain(|timestamp, _| *timestamp > pts);
        let width = output.data.width();
        let height = output.data.height();
        let mut info = video::VideoInfo::builder(video::VideoFormat::Nv12, width, height)
            .chroma_site(sample.chroma_site);
        if let Some(color) = &sample.color {
            info = info.colorimetry(color);
        }
        let info = info.build().map_err(MediaError::external)?;
        let mut format = yuv_format(&info, [2, 2], 8)
            .ok_or_else(|| MediaError::external("Unsupported GPU video color metadata"))?;
        if sample.color.is_none() {
            format.matrix = match output.metadata.color_space {
                gpu_video::parameters::ColorSpace::BT709 => YuvMatrix::Bt709,
                _ => YuvMatrix::Bt601,
            };
            format.full_range =
                output.metadata.color_range == gpu_video::parameters::ColorRange::Full;
        }
        Ok(DecodedFrame {
            presentation_time: Duration::from_nanos(pts),
            duration: sample.duration,
            frame: VisualFrame {
                width,
                height,
                data: FrameData::Video(Arc::new(VideoFrame::from_gpu(
                    info,
                    &self.device,
                    output.data,
                    format,
                ))),
            },
        })
    }
}
