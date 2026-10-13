//! Convert gpu-video's NV12 textures into the renderer's owned video texture.
use wgpu::util::DeviceExt;

use super::{RenderError, YuvConversionPipeline};
use crate::engine::frame::YuvFormat;

pub(super) struct VideoPlanePipeline(YuvConversionPipeline);

impl VideoPlanePipeline {
    pub(super) fn new(device: &wgpu::Device) -> Result<Self, RenderError> {
        Ok(Self(YuvConversionPipeline::new(
            device,
            "zerium.gpu.video",
            include_str!("nv12_input.wgsl"),
        )?))
    }

    pub(super) fn convert(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        planes: gpu_video::VideoTexture,
        format: YuvFormat,
        target: &wgpu::Texture,
    ) {
        let views = planes.planes().map(|plane| plane.create_view(None));
        let uniform = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("zerium-gpu-video-color"),
            contents: bytemuck::bytes_of(&format.conversion()),
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let binding = self.0.binding(device, views.iter(), &uniform);
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("zerium-gpu-video-convert"),
        });
        self.0.draw(&mut encoder, &binding, target);
        queue.submit([encoder.finish()]);
        queue.on_submitted_work_done(move || drop(planes));
    }
}
