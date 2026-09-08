use std::{
    collections::VecDeque,
    sync::{Arc, OnceLock, mpsc},
};

use super::{FrameRenderer, OUTPUT_FORMAT, RenderError, RenderScene, RenderSize};

struct ReadbackSlot {
    texture: wgpu::Texture,
    view: wgpu::TextureView,
    staging: wgpu::Buffer,
}

struct PendingReadback {
    frame_index: u64,
    slot: ReadbackSlot,
    submission: wgpu::SubmissionIndex,
    mapped: mpsc::Receiver<Result<(), wgpu::BufferAsyncError>>,
}

/// Keeps a small number of reusable GPU readback targets in flight.
pub(crate) struct ExportFramePipeline {
    renderer: Arc<FrameRenderer>,
    size: RenderSize,
    extent: wgpu::Extent3d,
    row_bytes: u32,
    padded_row_bytes: u32,
    available: Vec<ReadbackSlot>,
    pending: VecDeque<PendingReadback>,
}

impl ExportFramePipeline {
    pub(crate) fn new(
        renderer: Arc<FrameRenderer>,
        size: RenderSize,
        depth: usize,
    ) -> Result<Self, RenderError> {
        if size.width == 0 || size.height == 0 {
            return Err(RenderError::backend("export dimensions must be non-zero"));
        }
        // Export owns a session cache even when the caller passes the preview session.
        let renderer = Arc::new(renderer.fork());
        let extent = wgpu::Extent3d {
            width: size.width,
            height: size.height,
            depth_or_array_layers: 1,
        };
        let row_bytes = size
            .width
            .checked_mul(4)
            .ok_or_else(|| RenderError::backend("export row size is too large"))?;
        let alignment = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let padded_row_bytes = row_bytes
            .div_ceil(alignment)
            .checked_mul(alignment)
            .ok_or_else(|| RenderError::backend("padded export row size is too large"))?;
        let buffer_size = u64::from(padded_row_bytes)
            .checked_mul(u64::from(size.height))
            .ok_or_else(|| RenderError::backend("export frame is too large"))?;
        let available = (0..depth.max(1))
            .map(|_| {
                let texture = renderer.device.create_texture(&wgpu::TextureDescriptor {
                    label: Some("zerium-export-frame"),
                    size: extent,
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: OUTPUT_FORMAT,
                    usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
                    view_formats: &[],
                });
                let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
                let staging = renderer.device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("zerium-export-readback"),
                    size: buffer_size,
                    usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                });
                ReadbackSlot {
                    texture,
                    view,
                    staging,
                }
            })
            .collect();
        Ok(Self {
            renderer,
            size,
            extent,
            row_bytes,
            padded_row_bytes,
            available,
            pending: VecDeque::with_capacity(depth.max(1)),
        })
    }

    /// Submits one frame and returns the oldest frame when all slots are occupied.
    pub(crate) fn submit(
        &mut self,
        frame_index: u64,
        scene: &RenderScene,
    ) -> Result<Option<(u64, Vec<u8>)>, RenderError> {
        if scene.size != self.size {
            return Err(RenderError::backend(
                "export scene dimensions changed during rendering",
            ));
        }
        let ready = if self.available.is_empty() {
            self.finish_next()?
        } else {
            None
        };
        let slot = self
            .available
            .pop()
            .expect("a completed readback made one slot available");
        self.renderer.render_to_view(scene, &slot.view)?;
        let mut encoder =
            self.renderer
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("zerium-export-readback-encoder"),
                });
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &slot.texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &slot.staging,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(self.padded_row_bytes),
                    rows_per_image: Some(self.size.height),
                },
            },
            self.extent,
        );
        let submission = self.renderer.queue.submit([encoder.finish()]);
        let (sender, mapped) = mpsc::sync_channel(1);
        slot.staging
            .slice(..)
            .map_async(wgpu::MapMode::Read, move |result| {
                let _ = sender.send(result);
            });
        self.pending.push_back(PendingReadback {
            frame_index,
            slot,
            submission,
            mapped,
        });
        Ok(ready)
    }

    pub(crate) fn finish_next(&mut self) -> Result<Option<(u64, Vec<u8>)>, RenderError> {
        let Some(pending) = self.pending.pop_front() else {
            return Ok(None);
        };
        self.renderer
            .device
            .poll(wgpu::PollType::Wait {
                submission_index: Some(pending.submission),
                timeout: None,
            })
            .map_err(|error| RenderError::backend(format!("GPU readback failed: {error}")))?;
        pending
            .mapped
            .recv()
            .map_err(|_| RenderError::backend("GPU readback callback was dropped"))?
            .map_err(|error| RenderError::backend(format!("GPU frame mapping failed: {error}")))?;

        let mapped = pending
            .slot
            .staging
            .slice(..)
            .get_mapped_range()
            .map_err(|error| RenderError::backend(format!("GPU frame access failed: {error}")))?;
        let row_size = usize::try_from(self.row_bytes)
            .map_err(|_| RenderError::backend("export row size is too large"))?;
        let padded_row_size = usize::try_from(self.padded_row_bytes)
            .map_err(|_| RenderError::backend("padded export row size is too large"))?;
        let capacity = row_size
            .checked_mul(
                usize::try_from(self.size.height)
                    .map_err(|_| RenderError::backend("export height is too large"))?,
            )
            .ok_or_else(|| RenderError::backend("export frame is too large"))?;
        let mut rgba = Vec::with_capacity(capacity);
        for row in mapped.chunks_exact(padded_row_size) {
            rgba.extend_from_slice(&row[..row_size]);
        }
        drop(mapped);
        pending.slot.staging.unmap();
        decode_preview_surface_encoding_in_place(&mut rgba);
        let frame_index = pending.frame_index;
        self.available.push(pending.slot);
        Ok(Some((frame_index, rgba)))
    }
}

/// The preview surface stores a second sRGB encoding so GPUI's texture decode
/// yields display-code values. Export bypasses GPUI, so remove that outer
/// encoding before passing conventional RGBA bytes to FFmpeg.
fn decode_preview_surface_encoding_in_place(rgba: &mut [u8]) {
    static SRGB_TO_LINEAR: OnceLock<[u8; 256]> = OnceLock::new();
    let lookup = SRGB_TO_LINEAR.get_or_init(|| {
        std::array::from_fn(|value| {
            let encoded = value as f32 / 255.;
            let linear = if encoded <= 0.04045 {
                encoded / 12.92
            } else {
                ((encoded + 0.055) / 1.055).powf(2.4)
            };
            (linear * 255.).round() as u8
        })
    });
    for pixel in rgba.as_chunks_mut::<4>().0 {
        pixel[0] = lookup[pixel[0] as usize];
        pixel[1] = lookup[pixel[1] as usize];
        pixel[2] = lookup[pixel[2] as usize];
    }
}
