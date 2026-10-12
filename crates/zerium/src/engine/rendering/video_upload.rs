use super::{RenderError, pipelines::VideoInputPipeline};
use crate::engine::frame::{PixelPlane, YuvFormat};
use wgpu::util::DeviceExt;

pub(super) struct YuvUploadPipeline(VideoInputPipeline);

pub(super) struct YuvUpload {
    planes: [wgpu::Texture; 3],
    uniform: wgpu::Buffer,
    binding: wgpu::BindGroup,
    shape: (u32, u32, [u32; 2], u32),
}

impl YuvUploadPipeline {
    pub(super) fn new(device: &wgpu::Device) -> Result<Self, RenderError> {
        Ok(Self(VideoInputPipeline::new(
            device,
            "zerium.yuv.input",
            include_str!("yuv_input.wgsl"),
        )?))
    }

    pub(super) fn create_upload(
        &self,
        device: &wgpu::Device,
        width: u32,
        height: u32,
        yuv: &YuvFormat,
    ) -> YuvUpload {
        let format = if yuv.bit_depth > 8 {
            wgpu::TextureFormat::R16Uint
        } else {
            wgpu::TextureFormat::R8Uint
        };
        let planes = std::array::from_fn(|plane| {
            let [width, height] = yuv.plane_size(width, height, plane);
            device.create_texture(&wgpu::TextureDescriptor {
                label: Some("zerium-yuv-input-plane"),
                size: wgpu::Extent3d {
                    width,
                    height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            })
        });
        let views: [_; 3] =
            std::array::from_fn(|plane| planes[plane].create_view(&Default::default()));
        let uniform = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("zerium-yuv-input-conversion"),
            contents: bytemuck::bytes_of(&yuv.conversion()),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        let binding = self.0.binding(device, views.iter(), &uniform);
        YuvUpload {
            planes,
            uniform,
            binding,
            shape: (width, height, yuv.subsampling, yuv.bytes_per_sample()),
        }
    }

    pub(super) fn convert(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        upload: &YuvUpload,
        yuv: &YuvFormat,
        planes: &[PixelPlane<'_>; 3],
        output: &wgpu::Texture,
    ) {
        for (plane, texture) in upload.planes.iter().enumerate() {
            let size = texture.size();
            queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                planes[plane].bytes,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(planes[plane].stride),
                    rows_per_image: Some(size.height),
                },
                size,
            );
        }
        queue.write_buffer(&upload.uniform, 0, bytemuck::bytes_of(&yuv.conversion()));
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("zerium-yuv-input-encoder"),
        });
        self.0.draw(&mut encoder, &upload.binding, output);
        queue.submit([encoder.finish()]);
    }
}

impl YuvUpload {
    pub(super) fn matches(&self, width: u32, height: u32, yuv: &YuvFormat) -> bool {
        self.shape == (width, height, yuv.subsampling, yuv.bytes_per_sample())
    }
}
