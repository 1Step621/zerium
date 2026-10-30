//! Scene-linear RGB conversion to the output's pixel format.
use zerium_core::timeline::{ExportColorRange, ExportColorSettings, ExportColorSpace};

use crate::engine::{
    output::{EncodingError, VideoPixelFormat},
    processing::visual::{RenderScene, RenderSession, RenderSize, SCENE_FORMAT, texture_2d},
};

pub(super) fn plane_formats(format: VideoPixelFormat) -> &'static [wgpu::TextureFormat] {
    match format {
        VideoPixelFormat::Nv12 => &[wgpu::TextureFormat::R8Unorm, wgpu::TextureFormat::Rg8Unorm],
        VideoPixelFormat::P010 => &[wgpu::TextureFormat::R16Uint, wgpu::TextureFormat::Rg16Uint],
        VideoPixelFormat::Rgba => &[wgpu::TextureFormat::Rgba8UnormSrgb],
    }
}

/// Shared conversion from scene-linear RGB into YUV planes or straight sRGB.
pub(super) struct ColorConversion {
    view: wgpu::TextureView,
    binding: wgpu::BindGroup,
    pipelines: Vec<wgpu::RenderPipeline>,
}

impl ColorConversion {
    pub(super) fn new(
        device: &wgpu::Device,
        size: RenderSize,
        color: ExportColorSettings,
        format: VideoPixelFormat,
    ) -> Self {
        let source = texture_2d(
            device,
            size,
            SCENE_FORMAT,
            wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            "zerium-export-surface",
        );
        let view = source.create_view(&Default::default());
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("zerium-export-input"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: false },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            }],
        });
        let binding = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("zerium-export-input"),
            layout: &layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&view),
            }],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("zerium-export-color"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("zerium-export-color"),
            source: wgpu::ShaderSource::Wgsl(include_str!("color.wgsl").into()),
        });
        let constants = [
            (
                "COLOR_SPACE",
                match color.space {
                    ExportColorSpace::Bt709 => 0.,
                    ExportColorSpace::Bt2020 => 1.,
                    ExportColorSpace::Bt2100Pq => 2.,
                    ExportColorSpace::Bt2100Hlg => 3.,
                },
            ),
            (
                "FULL_RANGE",
                u8::from(color.range == ExportColorRange::Full) as f64,
            ),
            ("TEN_BIT", u8::from(format == VideoPixelFormat::P010) as f64),
        ];
        let pipelines = plane_formats(format)
            .iter()
            .enumerate()
            .map(|(plane, &plane_format)| {
                device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                    label: Some("zerium-export-color"),
                    layout: Some(&pipeline_layout),
                    vertex: wgpu::VertexState {
                        module: &shader,
                        entry_point: Some("vertex"),
                        compilation_options: Default::default(),
                        buffers: &[],
                    },
                    fragment: Some(wgpu::FragmentState {
                        module: &shader,
                        entry_point: Some(match (format, plane) {
                            (VideoPixelFormat::Nv12, 0) => "luma",
                            (VideoPixelFormat::Nv12, _) => "chroma",
                            (VideoPixelFormat::P010, 0) => "luma10",
                            (VideoPixelFormat::P010, _) => "chroma10",
                            (VideoPixelFormat::Rgba, _) => "rgba",
                        }),
                        compilation_options: wgpu::PipelineCompilationOptions {
                            constants: &constants,
                            ..Default::default()
                        },
                        targets: &[Some(wgpu::ColorTargetState {
                            format: plane_format,
                            blend: None,
                            write_mask: wgpu::ColorWrites::ALL,
                        })],
                    }),
                    primitive: Default::default(),
                    depth_stencil: None,
                    multisample: Default::default(),
                    multiview_mask: None,
                    cache: None,
                })
            })
            .collect();
        Self {
            view,
            binding,
            pipelines,
        }
    }

    pub(super) fn render(
        &self,
        renderer: &RenderSession,
        scene: &RenderScene,
        views: &[wgpu::TextureView],
    ) -> Result<wgpu::CommandEncoder, EncodingError> {
        renderer
            .submit_scene(scene, &self.view)
            .map_err(EncodingError::backend)?;
        let mut encoder =
            renderer
                .gpu()
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("zerium-export-color"),
                });
        for (pipeline, view) in self.pipelines.iter().zip(views) {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("zerium-export-color"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                    depth_slice: None,
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(pipeline);
            pass.set_bind_group(0, &self.binding, &[]);
            pass.draw(0..3, 0..1);
        }
        Ok(encoder)
    }
}
