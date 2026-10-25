//! Read a single composited scene pixel without UI overlays or display backgrounds.
use std::{sync::mpsc, time::Duration};

use super::RenderSession;
use crate::engine::processing::visual::RenderError;

pub(crate) struct ColorReadback {
    device: wgpu::Device,
    buffer: wgpu::Buffer,
    submission: wgpu::SubmissionIndex,
    mapped: mpsc::Receiver<Result<(), wgpu::BufferAsyncError>>,
}

impl RenderSession {
    /// Copy the displayed pixel before another preview frame can replace it.
    pub(crate) fn read_color(&self, position: [f32; 2]) -> Result<ColorReadback, RenderError> {
        if position
            .iter()
            .any(|value| !value.is_finite() || !(0. ..1.).contains(value))
        {
            return Err(RenderError::backend("Color sample is outside the preview"));
        }
        let state = self
            .state
            .lock()
            .map_err(|_| RenderError::backend("render state lock poisoned"))?;
        let frame = state
            .frame
            .as_ref()
            .ok_or_else(|| RenderError::backend("No preview frame is available"))?;
        let texture = &frame.scene.texture;
        let buffer = self
            .shared
            .gpu
            .device
            .create_buffer(&wgpu::BufferDescriptor {
                label: Some("zerium-color-sample"),
                size: 8,
                usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                mapped_at_creation: false,
            });
        let mut encoder = self
            .shared
            .gpu
            .device
            .create_command_encoder(&Default::default());
        let [x, y] = position;
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture,
                mip_level: 0,
                origin: wgpu::Origin3d {
                    x: ((x * texture.width() as f32) as u32).min(texture.width() - 1),
                    y: ((y * texture.height() as f32) as u32).min(texture.height() - 1),
                    z: 0,
                },
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout::default(),
            },
            wgpu::Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
        );
        let (sender, receiver) = mpsc::channel();
        encoder.map_buffer_on_submit(&buffer, wgpu::MapMode::Read, .., move |result| {
            let _ = sender.send(result);
        });
        let submission = self.shared.gpu.queue.submit([encoder.finish()]);
        // Submission order protects the copied pixel from subsequent frame renders.
        drop(state);
        Ok(ColorReadback {
            device: self.shared.gpu.device.clone(),
            buffer,
            submission,
            mapped: receiver,
        })
    }
}

impl ColorReadback {
    /// Wait for the copied pixel on a background thread, then return straight sRGB.
    pub(crate) fn finish(self) -> Result<[f32; 4], RenderError> {
        self.device
            .poll(wgpu::PollType::Wait {
                submission_index: Some(self.submission),
                timeout: Some(Duration::from_secs(5)),
            })
            .map_err(|error| RenderError::backend(error.to_string()))?;
        self.mapped
            .recv()
            .map_err(|error| RenderError::backend(error.to_string()))?
            .map_err(|error| RenderError::backend(error.to_string()))?;
        let mapped = self
            .buffer
            .slice(..)
            .get_mapped_range()
            .map_err(|error| RenderError::backend(error.to_string()))?;
        let color: [f32; 4] = std::array::from_fn(|channel| {
            let offset = channel * 2;
            half::f16::from_le_bytes([mapped[offset], mapped[offset + 1]]).to_f32()
        });
        drop(mapped);
        self.buffer.unmap();
        if !color.iter().all(|value| value.is_finite()) {
            return Err(RenderError::backend(
                "Preview pixel contains a non-finite color",
            ));
        }
        let [r, g, b, alpha] = color;
        if alpha <= 0.00001 {
            return Ok([0.; 4]);
        }
        Ok([
            display_channel(r / alpha),
            display_channel(g / alpha),
            display_channel(b / alpha),
            alpha.clamp(0., 1.),
        ])
    }
}

fn display_channel(linear: f32) -> f32 {
    let linear = linear.clamp(0., 1.);
    if linear <= 0.0031308 {
        linear * 12.92
    } else {
        1.055 * linear.powf(1. / 2.4) - 0.055
    }
}
