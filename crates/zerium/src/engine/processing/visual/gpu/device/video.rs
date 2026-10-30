//! Convert CPU pixels and imported video planes into owned linear RGB textures.
use std::{collections::BTreeMap, sync::Mutex};
use wgpu::util::DeviceExt;
use zerium_shader::WgslModule;

use super::shaders::raster_pipeline;
use crate::engine::{
    input::{PixelPlane, VideoColor, YuvFormat},
    processing::visual::{RenderError, RenderSize, SCENE_FORMAT, texture_2d},
};

pub(in crate::engine::processing::visual::gpu) struct YuvUploadPipeline(VideoConversionPipeline);

pub(in crate::engine::processing::visual::gpu) struct YuvUpload {
    planes: [wgpu::Texture; 3],
    uniform: wgpu::Buffer,
    binding: wgpu::BindGroup,
    shape: (u32, u32, [u32; 2], u32),
    color: VideoColor,
}

impl YuvUploadPipeline {
    pub(in crate::engine::processing::visual::gpu) fn new(
        device: &wgpu::Device,
    ) -> Result<Self, RenderError> {
        Ok(Self(VideoConversionPipeline::new(
            device,
            "zerium.yuv.input",
            include_str!("wgsl/yuv_input.wgsl"),
        )?))
    }

    pub(in crate::engine::processing::visual::gpu) fn create_upload(
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
            texture_2d(
                device,
                RenderSize { width, height },
                format,
                wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                "zerium-yuv-input-plane",
            )
        });
        let views: [_; 3] =
            std::array::from_fn(|plane| planes[plane].create_view(&Default::default()));
        let uniform = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("zerium-yuv-input-conversion"),
            contents: bytemuck::bytes_of(&yuv.conversion()),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        let binding = self.0.binding(device, views.iter(), &uniform, yuv.color);
        YuvUpload {
            planes,
            uniform,
            binding,
            shape: (width, height, yuv.subsampling, yuv.bytes_per_sample()),
            color: yuv.color,
        }
    }

    pub(in crate::engine::processing::visual::gpu) fn convert(
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
        self.0.submit(device, queue, &upload.binding, output);
    }
}

impl YuvUpload {
    pub(in crate::engine::processing::visual::gpu) fn matches(
        &self,
        width: u32,
        height: u32,
        yuv: &YuvFormat,
    ) -> bool {
        self.shape == (width, height, yuv.subsampling, yuv.bytes_per_sample())
            && self.color == yuv.color
    }
}

pub(in crate::engine::processing::visual::gpu) struct VideoPlanePipeline(VideoConversionPipeline);

impl VideoPlanePipeline {
    pub(in crate::engine::processing::visual::gpu) fn new(
        device: &wgpu::Device,
    ) -> Result<Self, RenderError> {
        Ok(Self(VideoConversionPipeline::new(
            device,
            "zerium.gpu.video",
            include_str!("wgsl/nv12_input.wgsl"),
        )?))
    }

    pub(in crate::engine::processing::visual::gpu) fn convert(
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
        let binding = self.0.binding(device, views.iter(), &uniform, format.color);
        self.0.submit(device, queue, &binding, target);
        queue.on_submitted_work_done(move || drop(planes));
    }
}

/// CPU pixels and imported GPU surfaces share color decoding and fullscreen conversion.
/// wgpu derives the binding layout and uniform sizes directly from each shader.
struct VideoConversionPipeline {
    layout: wgpu::BindGroupLayout,
    pipeline: wgpu::RenderPipeline,
    color_lut: Mutex<Option<(VideoColor, wgpu::Buffer)>>,
}

impl VideoConversionPipeline {
    fn new(device: &wgpu::Device, label: &str, shader: &str) -> Result<Self, RenderError> {
        let shader = format!("{}\n{shader}", include_str!("wgsl/video_color.wgsl"));
        WgslModule::parse(label, shader.as_str())?.validate_render(
            label,
            "vertex_main",
            "fragment_main",
            &BTreeMap::new(),
        )?;
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
                format: SCENE_FORMAT,
                blend: None,
                write_mask: wgpu::ColorWrites::ALL,
            },
            label,
            &[],
        );
        let layout = pipeline.get_bind_group_layout(0);
        Ok(Self {
            layout,
            pipeline,
            color_lut: Mutex::new(None),
        })
    }

    fn binding<'a>(
        &self,
        device: &wgpu::Device,
        views: impl IntoIterator<Item = &'a wgpu::TextureView>,
        uniform: &wgpu::Buffer,
        color: VideoColor,
    ) -> wgpu::BindGroup {
        let mut cached = self.color_lut.lock().unwrap_or_else(|e| e.into_inner());
        if cached
            .as_ref()
            .is_none_or(|(previous, _)| *previous != color)
        {
            let buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("zerium-video-transfer"),
                contents: bytemuck::cast_slice(&color.transfer_lut()),
                usage: wgpu::BufferUsages::STORAGE,
            });
            *cached = Some((color, buffer));
        }
        let (_, color_lut) = cached.as_ref().expect("transfer table was prepared");
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
            resource: color_lut.as_entire_binding(),
        });
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

    fn submit(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        binding: &wgpu::BindGroup,
        output: &wgpu::Texture,
    ) {
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("zerium-video-input-encoder"),
        });
        let view = output.create_view(&Default::default());
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
        drop(pass);
        queue.submit([encoder.finish()]);
    }
}

pub(in crate::engine::processing::visual::gpu) struct RgbaUploadPipeline(VideoConversionPipeline);

impl RgbaUploadPipeline {
    pub(in crate::engine::processing::visual::gpu) fn new(
        device: &wgpu::Device,
    ) -> Result<Self, RenderError> {
        Ok(Self(VideoConversionPipeline::new(
            device,
            "zerium.rgba.input",
            include_str!("wgsl/rgba_input.wgsl"),
        )?))
    }

    pub(in crate::engine::processing::visual::gpu) fn convert(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        plane: PixelPlane<'_>,
        color: VideoColor,
        output: &wgpu::Texture,
    ) {
        let source = texture_2d(
            device,
            RenderSize {
                width: output.width(),
                height: output.height(),
            },
            wgpu::TextureFormat::Rgba8Unorm,
            wgpu::TextureUsages::COPY_DST | wgpu::TextureUsages::TEXTURE_BINDING,
            "zerium-rgba-video",
        );
        queue.write_texture(
            source.as_image_copy(),
            plane.bytes,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(plane.stride),
                rows_per_image: Some(output.height()),
            },
            output.size(),
        );
        let uniform = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("zerium-rgba-video-color"),
            contents: bytemuck::bytes_of(&color.conversion()),
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let view = source.create_view(&Default::default());
        let binding = self.0.binding(device, [&view], &uniform, color);
        self.0.submit(device, queue, &binding, output);
    }
}
