use gstreamer_video as video;
use std::sync::mpsc;

use super::{EncoderError, VideoEncoderSettings};

pub(super) struct Readback {
    info: video::VideoInfo,
    stride: u32,
    free: Vec<wgpu::Buffer>,
}

pub(super) struct Frame {
    buffer: wgpu::Buffer,
    mapped: mpsc::Receiver<Result<(), wgpu::BufferAsyncError>>,
}

impl Readback {
    pub(super) fn new(
        device: &wgpu::Device,
        settings: VideoEncoderSettings,
    ) -> Result<Self, EncoderError> {
        let colorimetry = "bt709"
            .parse::<video::VideoColorimetry>()
            .map_err(EncoderError::pipeline)?;
        let info = video::VideoInfo::builder(
            video::VideoFormat::Nv12,
            settings.output.width,
            settings.output.height,
        )
        .colorimetry(&colorimetry)
        .build()
        .map_err(EncoderError::pipeline)?;
        let stride = settings
            .output
            .width
            .next_multiple_of(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT);
        if u64::from(stride) * u64::from(info.height()) * 3 / 2 > device.limits().max_buffer_size {
            return Err(EncoderError::Invalid(
                "Export readback exceeds the GPU buffer size limit",
            ));
        }
        Ok(Self {
            stride,
            info,
            free: Vec::new(),
        })
    }

    pub(super) fn info(&self) -> video::VideoInfo {
        self.info.clone()
    }

    pub(super) fn copy(
        &mut self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        planes: &[wgpu::Texture],
    ) -> Frame {
        let buffer = self.free.pop().unwrap_or_else(|| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("zerium-export-readback"),
                size: u64::from(self.stride) * u64::from(self.info.height()) * 3 / 2,
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
                            u64::from(self.stride) * u64::from(self.info.height())
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
        Frame { buffer, mapped }
    }

    pub(super) fn finish(&mut self, frame: Frame) -> Result<Vec<u8>, EncoderError> {
        frame
            .mapped
            .recv()
            .map_err(EncoderError::pipeline)?
            .map_err(EncoderError::pipeline)?;
        let mapped = frame
            .buffer
            .slice(..)
            .get_mapped_range()
            .map_err(EncoderError::pipeline)?;
        let mut data = vec![0; self.info.size()];
        for plane in 0..2 {
            let rows = self.info.height() as usize / if plane == 0 { 1 } else { 2 };
            let source_offset = if plane == 0 {
                0
            } else {
                self.stride as usize * self.info.height() as usize
            };
            for row in 0..rows {
                let source = source_offset + row * self.stride as usize;
                let destination =
                    self.info.offset()[plane] + row * self.info.stride()[plane] as usize;
                let width = self.info.width() as usize;
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
