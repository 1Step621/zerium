use std::sync::mpsc;

use super::{EncodingError, EncodingSettings};

pub(super) struct Nv12Readback {
    dimensions: super::super::encoding::VideoDimensions,
    frame_stride: usize,
    stride: u32,
    free: Vec<wgpu::Buffer>,
}

pub(super) struct PendingReadback {
    buffer: wgpu::Buffer,
    mapped: mpsc::Receiver<Result<(), wgpu::BufferAsyncError>>,
}

impl Nv12Readback {
    pub(super) fn new(
        device: &wgpu::Device,
        settings: EncodingSettings,
    ) -> Result<Self, EncodingError> {
        let dimensions = settings.output;
        let frame_stride = dimensions.width.next_multiple_of(4) as usize;
        let stride = settings
            .output
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
        })
    }

    pub(super) fn copy(
        &mut self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        planes: &[wgpu::Texture],
    ) -> PendingReadback {
        let buffer = self.free.pop().unwrap_or_else(|| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("zerium-export-readback"),
                size: u64::from(self.stride) * u64::from(self.dimensions.height) * 3 / 2,
                usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                mapped_at_creation: false,
            })
        });
        for (plane, texture) in planes.iter().enumerate() {
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

    pub(super) fn finish(&mut self, frame: PendingReadback) -> Result<Vec<u8>, EncodingError> {
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
