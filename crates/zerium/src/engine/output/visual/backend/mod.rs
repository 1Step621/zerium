//! Export adapters selected by the available encoding backend.
use crate::engine::{
    output::{
        VideoDimensions,
        visual::{
            Cancellation, ColorConversion, EncodingError, PendingFrame, VideoEncoderFrame, texture,
            wait_for_submission,
        },
    },
    processing::visual::{RenderScene, RenderSession, RenderSize},
};
use std::{collections::VecDeque, sync::mpsc};

#[cfg(any(target_os = "linux", target_os = "windows"))]
pub(super) mod gpu_video;
pub(super) mod gstreamer;
#[cfg(target_os = "macos")]
pub(super) mod video_toolbox;
pub(super) enum Backend {
    Readback(Nv12Readback),
    #[cfg(any(target_os = "linux", target_os = "windows"))]
    GpuVideo(gpu_video::Encoder),
    #[cfg(target_os = "macos")]
    VideoToolbox(video_toolbox::Encoder),
}

pub(super) struct Nv12Readback {
    dimensions: VideoDimensions,
    frame_stride: usize,
    stride: u32,
    free: Vec<wgpu::Buffer>,
    planes: [wgpu::Texture; 2],
    pending: VecDeque<PendingFrame<PendingReadback>>,
}

struct PendingReadback {
    buffer: wgpu::Buffer,
    mapped: mpsc::Receiver<Result<(), wgpu::BufferAsyncError>>,
}

impl Nv12Readback {
    pub(super) fn new(
        device: &wgpu::Device,
        dimensions: VideoDimensions,
    ) -> Result<Self, EncodingError> {
        let size = RenderSize {
            width: dimensions.width,
            height: dimensions.height,
        };
        let frame_stride = dimensions.width.next_multiple_of(4) as usize;
        let stride = dimensions
            .width
            .next_multiple_of(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT);
        if u64::from(stride) * u64::from(dimensions.height) * 3 / 2
            > device.limits().max_buffer_size
        {
            return Err(EncodingError::Invalid(
                "Export readback exceeds the GPU buffer size limit",
            ));
        }
        Ok(Self {
            stride,
            dimensions,
            frame_stride,
            free: Vec::new(),
            planes: std::array::from_fn(|plane| {
                let size = if plane == 0 {
                    size
                } else {
                    RenderSize {
                        width: size.width / 2,
                        height: size.height / 2,
                    }
                };
                texture(
                    device,
                    size,
                    crate::engine::output::visual::PLANE_FORMATS[plane],
                    wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
                )
            }),
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
            .each_ref()
            .map(|plane| plane.create_view(&Default::default()));
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

    fn copy(
        &mut self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
    ) -> PendingReadback {
        let buffer = self.free.pop().unwrap_or_else(|| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("zerium-export-readback"),
                size: u64::from(self.stride) * u64::from(self.dimensions.height) * 3 / 2,
                usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                mapped_at_creation: false,
            })
        });
        for (plane, texture) in self.planes.iter().enumerate() {
            encoder.copy_texture_to_buffer(
                texture.as_image_copy(),
                wgpu::TexelCopyBufferInfo {
                    buffer: &buffer,
                    layout: wgpu::TexelCopyBufferLayout {
                        offset: if plane == 0 {
                            0
                        } else {
                            u64::from(self.stride) * u64::from(self.dimensions.height)
                        },
                        bytes_per_row: Some(self.stride),
                        rows_per_image: None,
                    },
                },
                texture.size(),
            );
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
        let mut data = vec![0; self.frame_stride * self.dimensions.height as usize * 3 / 2];
        for plane in 0..2 {
            let rows = self.dimensions.height as usize / if plane == 0 { 1 } else { 2 };
            let source_offset = if plane == 0 {
                0
            } else {
                self.stride as usize * self.dimensions.height as usize
            };
            for row in 0..rows {
                let source = source_offset + row * self.stride as usize;
                let destination = if plane == 0 {
                    0
                } else {
                    self.frame_stride * self.dimensions.height as usize
                } + row * self.frame_stride;
                let width = self.dimensions.width as usize;
                data[destination..destination + width]
                    .copy_from_slice(&mapped[source..source + width]);
            }
        }
        drop(mapped);
        frame.buffer.unmap();
        self.free.push(frame.buffer);
        Ok(data)
    }
}
