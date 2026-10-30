//! Export adapters selected by the available encoding backend.
use crate::engine::{
    output::{
        EncodingSettings, VideoDimensions, VideoEncoderInput, VideoPixelFormat,
        visual::{
            Cancellation, ColorConversion, EncodingError, PendingFrame, VideoEncoderFrame,
            wait_for_submission,
        },
    },
    processing::visual::{RenderScene, RenderSession, RenderSize, texture_2d},
};
use std::{
    collections::VecDeque,
    path::Path,
    sync::{atomic::AtomicBool, mpsc},
};
use zerium_core::timeline::ExportContainer;

mod gif;
#[cfg(any(target_os = "linux", target_os = "windows"))]
pub(super) mod gpu_video;
pub(super) mod gstreamer;
#[cfg(target_os = "macos")]
pub(super) mod video_toolbox;

pub(crate) enum MediaWriter<'a> {
    Gstreamer(gstreamer::MediaWriter<'a>),
    Gif(gif::Writer<'a>),
}

impl<'a> MediaWriter<'a> {
    pub(crate) fn create(
        path: &Path,
        settings: EncodingSettings,
        encoding: VideoEncoderInput,
        cancelled: &'a AtomicBool,
    ) -> Result<Self, EncodingError> {
        if settings.container == ExportContainer::Gif {
            if let VideoEncoderInput::Raw(VideoPixelFormat::Rgba) = encoding {
                return gif::Writer::create(path, settings, cancelled).map(Self::Gif);
            }
            return Err(EncodingError::Invalid("GIF output requires RGBA frames"));
        }
        gstreamer::MediaWriter::create(path, settings, encoding, cancelled).map(Self::Gstreamer)
    }

    pub(crate) fn write_video(&mut self, frame: VideoEncoderFrame) -> Result<(), EncodingError> {
        match self {
            Self::Gstreamer(writer) => writer.write_video(frame),
            Self::Gif(writer) => writer.write_frame(frame),
        }
    }

    pub(crate) fn write_audio(&mut self, samples: &[f32]) -> Result<(), EncodingError> {
        match self {
            Self::Gstreamer(writer) => writer.write_audio(samples),
            Self::Gif(_) => Err(EncodingError::Invalid("GIF output has no audio stream")),
        }
    }

    pub(crate) fn finish(self) -> Result<(), EncodingError> {
        match self {
            Self::Gstreamer(writer) => writer.finish(),
            Self::Gif(writer) => writer.finish(),
        }
    }
}

pub(super) enum Backend {
    Readback(Readback),
    #[cfg(any(target_os = "linux", target_os = "windows"))]
    GpuVideo(gpu_video::Encoder),
    #[cfg(target_os = "macos")]
    VideoToolbox(video_toolbox::Encoder),
}

pub(super) struct Readback {
    frame_stride: usize,
    row_bytes: usize,
    stride: u32,
    free: Vec<wgpu::Buffer>,
    planes: Vec<wgpu::Texture>,
    pending: VecDeque<PendingFrame<PendingReadback>>,
}

struct PendingReadback {
    buffer: wgpu::Buffer,
    mapped: mpsc::Receiver<Result<(), wgpu::BufferAsyncError>>,
}

impl Readback {
    pub(super) fn new(
        device: &wgpu::Device,
        dimensions: VideoDimensions,
        format: VideoPixelFormat,
    ) -> Result<Self, EncodingError> {
        let size = RenderSize {
            width: dimensions.width,
            height: dimensions.height,
        };
        let row_bytes = format.row_bytes(dimensions.width);
        let frame_stride = row_bytes.next_multiple_of(4) as usize;
        let stride = row_bytes.next_multiple_of(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT);
        let planes: Vec<_> = crate::engine::output::visual::plane_formats(format)
            .iter()
            .enumerate()
            .map(|(plane, &format)| {
                let size = if plane == 0 {
                    size
                } else {
                    RenderSize {
                        width: size.width / 2,
                        height: size.height / 2,
                    }
                };
                texture_2d(
                    device,
                    size,
                    format,
                    wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
                    "zerium-export-surface",
                )
            })
            .collect();
        let rows: u64 = planes.iter().map(|plane| u64::from(plane.height())).sum();
        if u64::from(stride) * rows > device.limits().max_buffer_size {
            return Err(EncodingError::Invalid(
                "Export readback exceeds the GPU buffer size limit",
            ));
        }
        Ok(Self {
            stride,
            frame_stride,
            row_bytes: row_bytes as usize,
            free: Vec::new(),
            planes,
            pending: VecDeque::new(),
        })
    }

    pub(super) fn submit(
        &mut self,
        index: u64,
        renderer: &RenderSession,
        conversion: &ColorConversion,
        scene: &RenderScene,
    ) -> Result<(), EncodingError> {
        let views = self
            .planes
            .iter()
            .map(|plane| plane.create_view(&Default::default()))
            .collect::<Vec<_>>();
        let mut encoder = conversion.render(renderer, scene, &views)?;
        let data = self.copy(&renderer.gpu().device, &mut encoder);
        let submission = renderer.gpu().queue.submit([encoder.finish()]);
        self.pending.push_back(PendingFrame {
            index,
            data,
            submission,
        });
        Ok(())
    }

    pub(super) fn pending_frames(&self) -> usize {
        self.pending.len()
    }

    pub(super) fn finish_next(
        &mut self,
        device: &wgpu::Device,
        cancelled: Cancellation<'_>,
    ) -> Result<Option<VideoEncoderFrame>, EncodingError> {
        let Some(pending) = self.pending.pop_front() else {
            return Ok(None);
        };
        wait_for_submission(device, pending.submission, cancelled)?;
        Ok(Some(VideoEncoderFrame {
            index: pending.index,
            data: self.finish(pending.data)?,
            keyframe: false,
        }))
    }

    fn rows(&self) -> usize {
        self.planes
            .iter()
            .map(|plane| plane.height() as usize)
            .sum()
    }

    fn copy(
        &mut self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
    ) -> PendingReadback {
        let buffer = self.free.pop().unwrap_or_else(|| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("zerium-export-readback"),
                size: u64::from(self.stride) * self.rows() as u64,
                usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                mapped_at_creation: false,
            })
        });
        let mut offset = 0;
        for texture in &self.planes {
            encoder.copy_texture_to_buffer(
                texture.as_image_copy(),
                wgpu::TexelCopyBufferInfo {
                    buffer: &buffer,
                    layout: wgpu::TexelCopyBufferLayout {
                        offset,
                        bytes_per_row: Some(self.stride),
                        rows_per_image: None,
                    },
                },
                texture.size(),
            );
            offset += u64::from(self.stride) * u64::from(texture.height());
        }
        let (sender, mapped) = mpsc::channel();
        encoder.map_buffer_on_submit(&buffer, wgpu::MapMode::Read, .., move |result| {
            // The export can be cancelled before its GPU submission completes.
            let _ = sender.send(result);
        });
        PendingReadback { buffer, mapped }
    }

    fn finish(&mut self, frame: PendingReadback) -> Result<Vec<u8>, EncodingError> {
        frame
            .mapped
            .recv()
            .map_err(EncodingError::backend)?
            .map_err(EncodingError::backend)?;
        let mapped = frame
            .buffer
            .slice(..)
            .get_mapped_range()
            .map_err(EncodingError::backend)?;
        let mut data = vec![0; self.frame_stride * self.rows()];
        for row in 0..self.rows() {
            let source = row * self.stride as usize;
            let destination = row * self.frame_stride;
            data[destination..destination + self.row_bytes]
                .copy_from_slice(&mapped[source..source + self.row_bytes]);
        }
        drop(mapped);
        frame.buffer.unmap();
        self.free.push(frame.buffer);
        Ok(data)
    }
}
