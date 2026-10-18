//! Upload CPU YUV planes or import GPU video frames into owned RGB textures.
use wgpu::util::DeviceExt;
use zerium_shader::validate_render_shader;

use super::{RenderError, pipelines::raster_pipeline};
use crate::engine::frame::{PixelPlane, YuvFormat};

pub(super) struct YuvUploadPipeline(YuvConversionPipeline);

pub(super) struct YuvUpload {
    planes: [wgpu::Texture; 3],
    uniform: wgpu::Buffer,
    binding: wgpu::BindGroup,
    shape: (u32, u32, [u32; 2], u32),
}

impl YuvUploadPipeline {
    pub(super) fn new(device: &wgpu::Device) -> Result<Self, RenderError> {
        Ok(Self(YuvConversionPipeline::new(
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

/// Both CPU planes and imported GPU surfaces use the same fullscreen conversion.
/// wgpu derives the binding layout and uniform sizes directly from each shader.
struct YuvConversionPipeline {
    layout: wgpu::BindGroupLayout,
    pipeline: wgpu::RenderPipeline,
}

impl YuvConversionPipeline {
    fn new(device: &wgpu::Device, label: &str, shader: &str) -> Result<Self, RenderError> {
        validate_render_shader(label, shader, "vertex_main", "fragment_main")?;
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some(label),
            source: wgpu::ShaderSource::Wgsl(shader.into()),
        });
        let pipeline = raster_pipeline(
            device,
            None,
            &module,
            ["vertex_main", "fragment_main"],
            wgpu::ColorTargetState {
                format: wgpu::TextureFormat::Rgba8Unorm,
                blend: None,
                write_mask: wgpu::ColorWrites::ALL,
            },
            label,
            &[],
        );
        let layout = pipeline.get_bind_group_layout(0);
        Ok(Self { layout, pipeline })
    }

    fn binding<'a>(
        &self,
        device: &wgpu::Device,
        views: impl IntoIterator<Item = &'a wgpu::TextureView>,
        uniform: &wgpu::Buffer,
    ) -> wgpu::BindGroup {
        let mut entries: Vec<_> = views
            .into_iter()
            .enumerate()
            .map(|(binding, view)| wgpu::BindGroupEntry {
                binding: binding as u32,
                resource: wgpu::BindingResource::TextureView(view),
            })
            .collect();
        entries.push(wgpu::BindGroupEntry {
            binding: entries.len() as u32,
            resource: uniform.as_entire_binding(),
        });
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("zerium-video-input-binding"),
            layout: &self.layout,
            entries: &entries,
        })
    }

    fn draw(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        binding: &wgpu::BindGroup,
        output: &wgpu::Texture,
    ) {
        let view = output.create_view(&wgpu::TextureViewDescriptor {
            format: Some(wgpu::TextureFormat::Rgba8Unorm),
            ..Default::default()
        });
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("zerium-video-input-pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                    store: wgpu::StoreOp::Store,
                },
            })],
            ..Default::default()
        });
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, binding, &[]);
        pass.draw(0..3, 0..1);
    }
}
