//! Persistent, bounded GStreamer pipelines for random-access video frames.
use std::{
    sync::{
        Arc, Mutex, OnceLock,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use gst::prelude::*;
use gstreamer as gst;
use gstreamer_video as video;
use video::prelude::*;
use zerium_core::media::{MediaAsset, MediaKind};

use super::super::{
    initialize,
    pipeline::{self, SinkPipeline},
};
use super::device::{self, DecoderDevice};
use crate::engine::frame::{
    ChromaLocation, FrameData, PixelFrame, PixelPlane, VideoPixels, YuvFormat, YuvMatrix,
};
use crate::engine::media::{DecodedVideoFrame, MediaError, VideoDecoderSession};

/// A decoder surface until first upload, then a renderer-owned texture. Keeping
/// decoder pool slots in the frame cache would eventually stall hardware decode.
pub(crate) struct VideoFrame {
    pub(crate) info: video::VideoInfo,
    #[cfg(target_os = "linux")]
    pub(in crate::engine::gstreamer) drm_modifier: Option<u64>,
    storage: Mutex<FrameStorage>,
    pub(in crate::engine::gstreamer) transfer: Arc<OnceLock<VideoTransfer>>,
}

/// Chosen once per decoder session, including across seeks and size changes.
pub(in crate::engine::gstreamer) enum VideoTransfer {
    Cpu,
    Gpu(wgpu::Device),
}

enum FrameStorage {
    Decoder(gst::Buffer),
    Texture {
        device: wgpu::Device,
        texture: wgpu::Texture,
    },
}

impl std::fmt::Debug for VideoFrame {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GStreamerVideoFrame")
            .field("info", &self.info)
            .finish_non_exhaustive()
    }
}

impl PartialEq for VideoFrame {
    fn eq(&self, other: &Self) -> bool {
        std::ptr::eq(self, other)
    }
}
impl Eq for VideoFrame {}

impl VideoFrame {
    pub(crate) fn buffer(&self) -> Option<gst::Buffer> {
        match &*self.storage.lock().unwrap_or_else(|e| e.into_inner()) {
            FrameStorage::Decoder(buffer) => Some(buffer.clone()),
            FrameStorage::Texture { .. } => None,
        }
    }

    pub(crate) fn texture(
        &self,
        device: &wgpu::Device,
    ) -> Result<Option<wgpu::Texture>, MediaError> {
        match &*self.storage.lock().unwrap_or_else(|e| e.into_inner()) {
            FrameStorage::Decoder(_) => Ok(None),
            FrameStorage::Texture {
                device: owner,
                texture,
            } if owner == device => Ok(Some(texture.clone())),
            FrameStorage::Texture { .. } => Err(MediaError::external(
                "A video frame cannot be reused on a different GPU device",
            )),
        }
    }

    pub(crate) fn retain_texture(&self, device: &wgpu::Device, texture: &wgpu::Texture) {
        *self.storage.lock().unwrap_or_else(|e| e.into_inner()) = FrameStorage::Texture {
            device: device.clone(),
            texture: texture.clone(),
        };
    }

    pub(crate) fn map_cpu_pixels(&self) -> Result<MappedVideoFrame, MediaError> {
        let buffer = self
            .buffer()
            .ok_or_else(|| MediaError::external("Video pixels are already GPU resident"))?;
        map_pixels(buffer, &self.info)
    }

    pub(crate) fn byte_len(&self) -> usize {
        // The cache accounts for the owned RGBA texture as well as source padding.
        self.info
            .size()
            .max(self.info.width() as usize * self.info.height() as usize * 4)
    }
}

pub(crate) struct VideoDecoder {
    asset: MediaAsset,
    pipeline: Option<SinkPipeline>,
    last: Option<DecodedVideoFrame>,
    lookahead: Option<DecodedVideoFrame>,
    device: Option<wgpu::Device>,
    decoder_device: Option<DecoderDevice>,
    transfer: Arc<OnceLock<VideoTransfer>>,
}

impl VideoDecoder {
    pub(crate) fn open(
        asset: MediaAsset,
        device: Option<&wgpu::Device>,
    ) -> Result<Self, MediaError> {
        initialize().map_err(MediaError::external)?;
        let decoder_device = device.and_then(DecoderDevice::for_device);
        if device.is_some() && decoder_device.is_none() {
            eprintln!("Video input uses software decoding: renderer GPU affinity is unavailable");
        }
        Ok(Self {
            asset,
            pipeline: None,
            last: None,
            lookahead: None,
            device: device.cloned(),
            decoder_device,
            transfer: Arc::new(OnceLock::new()),
        })
    }

    fn close(&mut self) {
        self.pipeline = None;
        self.last = None;
        self.lookahead = None;
    }

    fn configure(&mut self, cancelled: &AtomicBool) -> Result<(), MediaError> {
        if self.pipeline.is_some() {
            return Ok(());
        }
        let native = self.decoder_device.is_some()
            && !matches!(self.transfer.get(), Some(VideoTransfer::Cpu));
        if !native {
            self.transfer.get_or_init(|| VideoTransfer::Cpu);
        }
        let result = match self.build(native, cancelled) {
            Err(MediaError::Cancelled) => Err(MediaError::Cancelled),
            Err(error) if native && !matches!(self.transfer.get(), Some(VideoTransfer::Gpu(_))) => {
                eprintln!("Video input uses software decoding: {error}");
                self.close();
                self.transfer.get_or_init(|| VideoTransfer::Cpu);
                self.build(false, cancelled)
            }
            result => result,
        };
        if result.is_err() {
            self.close();
        }
        result
    }

    fn build(&mut self, native: bool, cancelled: &AtomicBool) -> Result<(), MediaError> {
        let pipeline = SinkPipeline::new();
        let decoder_device = self.decoder_device.as_ref().filter(|_| native);
        if let Some(device) = decoder_device {
            device.configure_pipeline(&pipeline.pipeline);
        }
        let uri = pipeline::file_uri(&self.asset.path)?;
        let source = gst::ElementFactory::make("uridecodebin")
            .property("uri", uri.as_str())
            .property("expose-all-streams", false)
            .property(
                "caps",
                gst::Caps::builder("video/x-raw").any_features().build(),
            )
            .build()
            .map_err(MediaError::external)?;
        device::configure_source(&source, decoder_device);
        #[cfg(target_os = "linux")]
        if native {
            pipeline.sink.set_callbacks(
                gstreamer_app::AppSinkCallbacks::builder()
                    .propose_allocation(|_, query| {
                        query.add_allocation_meta::<video::VideoMeta>(None);
                        true
                    })
                    .build(),
            );
        }
        let (width, height) = match self.asset.kind {
            MediaKind::Video { width, height, .. } if width > 0 && height > 0 => (width, height),
            _ => return Err(MediaError::invalid_input("Invalid video dimensions")),
        };
        let mut chain: Vec<gst::Element> = Vec::new();
        let mut caps = gst::Caps::builder("video/x-raw").features([] as [&str; 0]);
        if native {
            #[cfg(target_os = "linux")]
            {
                let postproc = decoder_device
                    .expect("native decoding has a renderer device")
                    .postprocessor()?;
                let supported = postproc
                    .static_pad("src")
                    .expect("vapostproc has a source pad")
                    .query_caps(None);
                let formats: Vec<String> = supported
                    .iter()
                    .filter_map(|structure| structure.value("drm-format").ok())
                    .flat_map(|value| {
                        value
                            .get::<gst::List>()
                            .map(|list| list.as_slice().to_vec())
                            .unwrap_or_else(|_| vec![value.clone()])
                    })
                    .filter_map(|value| value.get::<String>().ok())
                    .filter(|format| format.split(':').next() == Some("AR24"))
                    .collect();
                if formats.is_empty() {
                    return Err(MediaError::external("VA driver cannot export BGRA DMA-BUF"));
                }
                // Retain the driver's actual modifier; AMD normally uses tiles.
                caps = gst::Caps::builder("video/x-raw")
                    .features(["memory:DMABuf"])
                    .field("format", "DMA_DRM")
                    .field("drm-format", gst::List::new(formats));
                chain.push(postproc);
            }
            #[cfg(target_os = "windows")]
            {
                chain.push(
                    gst::ElementFactory::make("d3d12convert")
                        .build()
                        .map_err(MediaError::external)?,
                );
                caps = gst::Caps::builder("video/x-raw")
                    .features(["memory:D3D12Memory"])
                    .field("format", "RGBA");
            }
            #[cfg(target_os = "macos")]
            {
                if gst::version() < (1, 30, 0, 0) {
                    return Err(MediaError::external(
                        "Native IOSurface access requires GStreamer 1.30",
                    ));
                }
                // Keep VideoToolbox's IOSurface output intact before Metal import.
                caps = gst::Caps::builder("video/x-raw")
                    .features(["memory:IOSurface"])
                    .field("format", gst::List::new(["NV12", "P010_10LE"]));
            }
            #[cfg(not(any(target_os = "linux", target_os = "windows", target_os = "macos")))]
            return Err(MediaError::external(
                "No native video importer on this platform",
            ));
        } else {
            chain.push(
                gst::ElementFactory::make("videoconvertscale")
                    .build()
                    .map_err(MediaError::external)?,
            );
            caps = caps.field(
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
            );
        }
        if !native || !cfg!(target_os = "macos") {
            caps = caps
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
            if !native
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
                caps.structure(0)
                    .is_some_and(|s| s.name().starts_with("video/"))
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
        self.pipeline = Some(pipeline);
        self.pipeline
            .as_ref()
            .unwrap()
            .pipeline
            .set_state(gst::State::Paused)
            .map_err(MediaError::external)?;
        let sample = self
            .pull(true, cancelled)?
            .ok_or_else(|| MediaError::external("Video contains no frames"))?;
        // Validate import before publishing any frame. Non-linear DMA-BUFs
        // cannot be mapped for CPU fallback, so select CPU output here instead.
        // Preroll is delivered again as the first PLAYING sample.
        let decoded = self.frame(sample)?;
        if native && self.transfer.get().is_none() {
            let FrameData::Video(frame) = &decoded.frame.data else {
                unreachable!()
            };
            if crate::engine::gstreamer::gpu::NativeVideoPipeline::import(
                self.device
                    .as_ref()
                    .expect("native output has a GPU device"),
                frame,
            )
            .map_err(MediaError::external)?
            .is_none()
            {
                return Err(MediaError::external(
                    "Decoder surface cannot be imported on the rendering GPU",
                ));
            }
        }
        drop(decoded);
        self.pipeline
            .as_ref()
            .unwrap()
            .pipeline
            .set_state(gst::State::Playing)
            .map_err(MediaError::external)?;
        Ok(())
    }

    fn pull(
        &self,
        preroll: bool,
        cancelled: &AtomicBool,
    ) -> Result<Option<gst::Sample>, MediaError> {
        let pipeline = self
            .pipeline
            .as_ref()
            .ok_or_else(|| MediaError::external("Video pipeline is closed"))?;
        pipeline.pull(preroll, Some(cancelled))
    }

    fn frame(&self, sample: gst::Sample) -> Result<DecodedVideoFrame, MediaError> {
        let caps = sample
            .caps()
            .ok_or_else(|| MediaError::external("Video frame has no format"))?;
        let drm_info = if video::is_dma_drm_caps(caps) {
            Some(video::VideoInfoDmaDrm::from_caps(caps).map_err(MediaError::external)?)
        } else {
            None
        };
        let info = match &drm_info {
            Some(info) => info.to_video_info(),
            None => video::VideoInfo::from_caps(caps),
        }
        .map_err(MediaError::external)?;
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
        let frame = PixelFrame {
            width: info.width(),
            height: info.height(),
            data: FrameData::Video(Arc::new(VideoFrame {
                #[cfg(target_os = "linux")]
                drm_modifier: drm_info.map(|info| info.modifier()),
                info,
                storage: Mutex::new(FrameStorage::Decoder(buffer)),
                transfer: self.transfer.clone(),
            })),
        };
        Ok(DecodedVideoFrame {
            presentation_time,
            duration,
            frame,
        })
    }

    fn next(&mut self, cancelled: &AtomicBool) -> Result<Option<DecodedVideoFrame>, MediaError> {
        loop {
            let Some(sample) = self.pull(false, cancelled)? else {
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
            return self.frame(sample).map(Some);
        }
    }
    /// The following PTS defines the current frame's actual presentation end.
    /// Nominal duration can overlap the next frame after time-base rounding,
    /// and accurate seeks can produce two samples at the same clipped PTS.
    fn read_interval(
        &mut self,
        cancelled: &AtomicBool,
    ) -> Result<Option<DecodedVideoFrame>, MediaError> {
        let mut current = match self.lookahead.take() {
            Some(frame) => frame,
            None => {
                let Some(frame) = self.next(cancelled)? else {
                    return Ok(None);
                };
                frame
            }
        };
        loop {
            let Some(next) = self.next(cancelled)? else {
                // A container can keep playing audio after video ends. The
                // final picture covers that tail, including requests after a seek.
                current.duration = current.duration.max(
                    self.asset
                        .duration
                        .saturating_sub(current.presentation_time),
                );
                return Ok(Some(current));
            };
            if next.presentation_time <= current.presentation_time {
                current = next;
                continue;
            }
            current.duration = next
                .presentation_time
                .saturating_sub(current.presentation_time);
            self.lookahead = Some(next);
            return Ok(Some(current));
        }
    }

    fn seek(&mut self, time: Duration) -> Result<(), MediaError> {
        self.last = None;
        self.lookahead = None;
        // Read full presentation intervals instead of clipping at the target,
        // which can lie after the final picture in an audio-only tail.
        self.pipeline
            .as_ref()
            .unwrap()
            .sink
            .seek_simple(
                gst::SeekFlags::FLUSH | gst::SeekFlags::KEY_UNIT | gst::SeekFlags::SNAP_BEFORE,
                gst::ClockTime::from_nseconds(
                    u64::try_from(time.as_nanos()).map_err(MediaError::external)?,
                ),
            )
            .map_err(MediaError::external)
    }
}

impl VideoDecoderSession for VideoDecoder {
    fn decode_at(
        &mut self,
        time: Duration,
        cancelled: &AtomicBool,
    ) -> Result<DecodedVideoFrame, MediaError> {
        if cancelled.load(Ordering::Relaxed) {
            return Err(MediaError::Cancelled);
        }
        self.configure(cancelled)?;
        if let Some(last) = &self.last
            && last.covers(time)
        {
            return Ok(last.clone());
        }
        // Nearby forward requests consume the existing stream. Backward and
        // distant requests decode from the preceding keyframe to the target.
        let previous = self
            .last
            .as_ref()
            .map_or(Duration::ZERO, |last| last.presentation_time);
        let seek = self.last.as_ref().map_or(!time.is_zero(), |last| {
            time < last.presentation_time
                || time
                    > last
                        .presentation_time
                        .saturating_add(last.duration.saturating_mul(10))
        });
        if seek {
            self.seek(time)?;
        }
        let mut next = self.read_interval(cancelled)?;
        if next.is_none() && seek && time > previous {
            // Some demuxers return EOS even for a keyframe seek in the audio
            // tail. Read from a known earlier position to find the final picture.
            self.seek(previous)?;
            next = self.read_interval(cancelled)?;
        }
        while let Some(frame) = next {
            self.last = Some(frame.clone());
            if time.saturating_add(Duration::from_nanos(1))
                < frame.presentation_time.saturating_add(frame.duration)
            {
                return Ok(frame);
            }
            next = self.read_interval(cancelled)?;
        }
        self.last
            .clone()
            .ok_or_else(|| MediaError::external("No video frame at the requested time"))
    }
}

/// Owns the map so borrowed plane slices remain valid through queue.write_texture.
pub(crate) struct MappedVideoFrame(video::VideoFrame<video::video_frame::Readable>);

impl MappedVideoFrame {
    pub(crate) fn pixels(&self) -> Result<VideoPixels<'_>, MediaError> {
        let frame = &self.0;
        if let Some((subsampling, bit_depth)) = yuv_layout(frame.format()) {
            let format = yuv_format(frame.info(), subsampling, bit_depth)
                .ok_or_else(|| MediaError::external("Unsupported YUV color metadata"))?;
            let plane = |index| {
                let [width, height] = format.plane_size(frame.width(), frame.height(), index);
                self.plane(index, width * format.bytes_per_sample(), height)
            };
            Ok(VideoPixels::Yuv {
                format,
                planes: [plane(0)?, plane(1)?, plane(2)?],
            })
        } else {
            Ok(VideoPixels::Rgba(self.plane(
                0,
                frame.width() * 4,
                frame.height(),
            )?))
        }
    }

    fn plane(&self, index: usize, row_bytes: u32, rows: u32) -> Result<PixelPlane<'_>, MediaError> {
        let stride = u32::try_from(self.0.plane_stride()[index]).map_err(MediaError::external)?;
        let bytes = self
            .0
            .plane_data(index as u32)
            .map_err(MediaError::external)?;
        let required = (rows.saturating_sub(1) as usize)
            .checked_mul(stride as usize)
            .and_then(|size| size.checked_add(row_bytes as usize))
            .ok_or_else(|| MediaError::external("Video plane size overflow"))?;
        if row_bytes > stride || bytes.len() < required {
            return Err(MediaError::external("Invalid video plane layout"));
        }
        Ok(PixelPlane { bytes, stride })
    }
}

fn yuv_layout(format: video::VideoFormat) -> Option<([u32; 2], u32)> {
    use video::VideoFormat as F;
    match format {
        F::I420 => Some(([2, 2], 8)),
        F::I42010le => Some(([2, 2], 10)),
        F::I42012le => Some(([2, 2], 12)),
        F::Y42b => Some(([2, 1], 8)),
        F::I42210le => Some(([2, 1], 10)),
        F::I42212le => Some(([2, 1], 12)),
        F::Y444 => Some(([1, 1], 8)),
        F::Y44410le => Some(([1, 1], 10)),
        F::Y44412le => Some(([1, 1], 12)),
        _ => None,
    }
}

fn map_pixels(
    buffer: gst::Buffer,
    info: &video::VideoInfo,
) -> Result<MappedVideoFrame, MediaError> {
    use video::VideoFormat as F;
    let mapped = video::VideoFrame::from_buffer_readable(buffer, info)
        .map_err(|_| MediaError::external("Could not map decoded video pixels"))?;
    let info = mapped.info();
    if info.format() == F::Rgba
        || yuv_layout(info.format()).is_some_and(|(subsampling, bit_depth)| {
            yuv_format(info, subsampling, bit_depth).is_some()
        })
    {
        return Ok(MappedVideoFrame(mapped));
    }
    let format = match info.format() {
        F::Nv12 => F::I420,
        F::P01010le => F::I42010le,
        _ => F::Rgba,
    };
    let mut builder = video::VideoInfo::builder(format, mapped.width(), mapped.height());
    if format != F::Rgba {
        builder = builder
            .colorimetry(&info.colorimetry())
            .chroma_site(info.chroma_site());
    }
    let info = builder.build().map_err(MediaError::external)?;
    let buffer = gst::Buffer::with_size(info.size()).map_err(MediaError::external)?;
    let mut output = video::VideoFrame::from_buffer_writable(buffer, &info)
        .map_err(|_| MediaError::external("Could not map converted video pixels"))?;
    video::VideoConverter::new(mapped.info(), &info, None)
        .map_err(MediaError::external)?
        .frame(&mapped, &mut output);
    map_pixels(output.into_buffer(), &info)
}

/// Use the same color metadata for CPU planes and imported NV12 surfaces.
pub(in crate::engine::gstreamer) fn yuv_format(
    info: &video::VideoInfo,
    subsampling: [u32; 2],
    bit_depth: u32,
) -> Option<YuvFormat> {
    let color = info.colorimetry();
    let matrix = match color.matrix() {
        video::VideoColorMatrix::Bt709 => YuvMatrix::Bt709,
        video::VideoColorMatrix::Bt2020 => YuvMatrix::Bt2020,
        video::VideoColorMatrix::Bt601 | video::VideoColorMatrix::Unknown => YuvMatrix::Bt601,
        _ => return None,
    };
    let chroma = info.chroma_site();
    Some(YuvFormat {
        subsampling,
        bit_depth,
        matrix,
        full_range: color.range() == video::VideoColorRange::Range0_255,
        chroma_location: match (
            chroma.contains(video::VideoChromaSite::H_COSITED),
            chroma.contains(video::VideoChromaSite::V_COSITED),
        ) {
            (true, true) => ChromaLocation::TopLeft,
            (true, false) => ChromaLocation::Left,
            (false, true) => ChromaLocation::Top,
            (false, false) => ChromaLocation::Center,
        },
    })
}
