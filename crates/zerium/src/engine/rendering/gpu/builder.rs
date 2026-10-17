//! Build shared bind layouts and built-in rendering pipelines.

use super::bindings::{GpuComposite, GpuImage, GpuItem, GpuPass, GpuSurface};
use super::pipelines::raster_pipeline;
use super::{
    COMPOSITE, OUTPUT_FORMAT, PROPERTY_WORD_SIZE, RenderDevice, RenderDeviceBuilder, RenderError,
    SCENE_FORMAT,
};
use crate::engine::gpu::GpuContext;
use std::{collections::HashMap, num::NonZeroU64, sync::Arc};
use zerium_shader::{
    CompiledEffectShader, CompiledPluginShaders, IMAGE_SAMPLER_BINDING, ImageBinding,
    validate_render_shader,
};

impl RenderDeviceBuilder {
    pub(crate) fn new(gpu: GpuContext) -> Result<Self, RenderError> {
        let device = &gpu.device;
        let item_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("zerium-item-bind-group-layout"),
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Storage { read_only: true },
                            has_dynamic_offset: false,
                            min_binding_size: NonZeroU64::new(size_of::<GpuItem>() as u64),
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Storage { read_only: true },
                            has_dynamic_offset: false,
                            min_binding_size: NonZeroU64::new(PROPERTY_WORD_SIZE as u64),
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 2,
                        visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Uniform,
                            has_dynamic_offset: false,
                            min_binding_size: NonZeroU64::new(size_of::<GpuSurface>() as u64),
                        },
                        count: None,
                    },
                ],
            });
        let mut capability_entries = ImageBinding::all()
            .flat_map(|binding| {
                [
                    wgpu::BindGroupLayoutEntry {
                        binding: binding.texture(),
                        visibility: wgpu::ShaderStages::VERTEX_FRAGMENT
                            | wgpu::ShaderStages::COMPUTE,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Float { filterable: true },
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: binding.metadata(),
                        visibility: wgpu::ShaderStages::VERTEX_FRAGMENT
                            | wgpu::ShaderStages::COMPUTE,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Uniform,
                            has_dynamic_offset: false,
                            min_binding_size: NonZeroU64::new(size_of::<GpuImage>() as u64),
                        },
                        count: None,
                    },
                ]
            })
            .collect::<Vec<_>>();

        capability_entries.push(wgpu::BindGroupLayoutEntry {
            binding: IMAGE_SAMPLER_BINDING,
            visibility: wgpu::ShaderStages::VERTEX_FRAGMENT | wgpu::ShaderStages::COMPUTE,
            ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
            count: None,
        });

        let capability_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("zerium-capability-input-layout"),
                entries: &capability_entries,
            });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("zerium-frame-pipeline-layout"),
            bind_group_layouts: &[
                Some(&item_bind_group_layout),
                Some(&capability_bind_group_layout),
            ],
            immediate_size: 0,
        });
        let pass_entries = [
            wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT | wgpu::ShaderStages::COMPUTE,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: NonZeroU64::new(size_of::<GpuPass>() as u64),
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 1,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT | wgpu::ShaderStages::COMPUTE,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Storage { read_only: true },
                    has_dynamic_offset: false,
                    min_binding_size: NonZeroU64::new(PROPERTY_WORD_SIZE as u64),
                },
                count: None,
            },
        ];
        let pass_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("zerium-pass-layout"),
                entries: &pass_entries,
            });
        let mut compute_entries = pass_entries.to_vec();
        compute_entries.push(wgpu::BindGroupLayoutEntry {
            binding: 2,
            visibility: wgpu::ShaderStages::COMPUTE,
            ty: wgpu::BindingType::StorageTexture {
                access: wgpu::StorageTextureAccess::WriteOnly,
                format: SCENE_FORMAT,
                view_dimension: wgpu::TextureViewDimension::D2,
            },
            count: None,
        });
        let compute_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("zerium-compute-pass-layout"),
                entries: &compute_entries,
            });
        let composite_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("zerium-composite-bind-group-layout"),
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Float { filterable: true },
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 2,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Uniform,
                            has_dynamic_offset: false,
                            min_binding_size: NonZeroU64::new(size_of::<GpuComposite>() as u64),
                        },
                        count: None,
                    },
                ],
            });
        let pass_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("zerium-effect-pipeline-layout"),
            bind_group_layouts: &[
                Some(&pass_bind_group_layout),
                Some(&capability_bind_group_layout),
            ],
            immediate_size: 0,
        });
        let composite_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("zerium-composite-pipeline-layout"),
                bind_group_layouts: &[Some(&composite_bind_group_layout)],
                immediate_size: 0,
            });
        let backdrop_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("zerium-backdrop-bind-group-layout"),
                entries: &[wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                }],
            });
        let blend_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("zerium-blend-pipeline-layout"),
                bind_group_layouts: &[
                    Some(&composite_bind_group_layout),
                    Some(&backdrop_bind_group_layout),
                ],
                immediate_size: 0,
            });
        validate_render_shader(
            "zerium.composite",
            COMPOSITE,
            "vertex_main",
            "fragment_main",
        )?;
        validate_render_shader(
            "zerium.output_transform",
            COMPOSITE,
            "vertex_main",
            "output_fragment_main",
        )?;
        validate_render_shader(
            "zerium.blend",
            COMPOSITE,
            "vertex_main",
            "blend_fragment_main",
        )?;
        let composite_module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("zerium-composite-shader"),
            source: wgpu::ShaderSource::Wgsl(COMPOSITE.into()),
        });
        let composite_pipeline = raster_pipeline(
            device,
            &composite_pipeline_layout,
            &composite_module,
            ["vertex_main", "fragment_main"],
            wgpu::ColorTargetState {
                format: SCENE_FORMAT,
                blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                write_mask: wgpu::ColorWrites::ALL,
            },
            "zerium-composite_pipeline",
            &[],
        );
        let output_pipeline = raster_pipeline(
            device,
            &composite_pipeline_layout,
            &composite_module,
            ["vertex_main", "output_fragment_main"],
            wgpu::ColorTargetState {
                format: OUTPUT_FORMAT,
                blend: None,
                write_mask: wgpu::ColorWrites::ALL,
            },
            "zerium-output_pipeline",
            &[],
        );
        let blend_pipeline = raster_pipeline(
            device,
            &blend_pipeline_layout,
            &composite_module,
            ["vertex_main", "blend_fragment_main"],
            wgpu::ColorTargetState {
                format: SCENE_FORMAT,
                blend: None,
                write_mask: wgpu::ColorWrites::ALL,
            },
            "zerium-blend-pipeline",
            &[],
        );
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("zerium-effect-sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let frame_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("zerium-frame-image-layout"),
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Float { filterable: true },
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                        count: None,
                    },
                ],
            });
        let frame_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("zerium-frame-image-pipeline-layout"),
            bind_group_layouts: &[Some(&frame_bind_group_layout)],
            immediate_size: 0,
        });
        let frame_source = include_str!("frame.wgsl");
        validate_render_shader(
            "zerium.frame_image",
            frame_source,
            "vertex_main",
            "fragment_main",
        )?;
        let frame_module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("zerium-frame-image-shader"),
            source: wgpu::ShaderSource::Wgsl(frame_source.into()),
        });
        let frame_pipeline = raster_pipeline(
            device,
            &frame_layout,
            &frame_module,
            ["vertex_main", "fragment_main"],
            wgpu::ColorTargetState {
                format: SCENE_FORMAT,
                blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                write_mask: wgpu::ColorWrites::ALL,
            },
            "zerium-frame-image-pipeline",
            &[],
        );
        let yuv_upload = super::video_upload::YuvUploadPipeline::new(device)?;
        let video_planes = super::VideoPlanePipeline::new(device)?;
        Ok(Self {
            render_device: RenderDevice {
                gpu,
                pipeline_layout,
                item_pipelines: HashMap::new(),
                item_bind_group_layout,
                capability_bind_group_layout,
                pass_bind_group_layout,
                compute_bind_group_layout,
                pass_pipeline_layout,
                composite_bind_group_layout,
                backdrop_bind_group_layout,
                effect_pipelines: HashMap::new(),
                temporal_pipelines: HashMap::new(),
                compute_pipelines: HashMap::new(),
                composite_pipeline,
                blend_pipeline,
                output_pipeline,
                frame_pipeline,
                frame_bind_group_layout,
                sampler,
                yuv_upload,
                video_planes,
            },
        })
    }

    pub(crate) fn register_plugins(
        mut self,
        shaders: &CompiledPluginShaders,
    ) -> Result<Self, RenderError> {
        for descriptor in &shaders.items {
            self.render_device
                .register_item_shader(descriptor.clone())?;
        }
        for effect in &shaders.effects {
            match effect {
                CompiledEffectShader::Render(descriptor) => {
                    self.render_device
                        .register_effect_shader(descriptor.clone())?;
                }
                CompiledEffectShader::Compute(descriptor) => {
                    self.render_device
                        .register_compute_shader(descriptor.clone())?;
                }
                CompiledEffectShader::Temporal(descriptor) => {
                    self.render_device
                        .register_temporal_shader(descriptor.clone())?;
                }
            }
        }
        Ok(self)
    }

    pub(crate) fn build(self) -> Arc<RenderDevice> {
        Arc::new(self.render_device)
    }
}

impl RenderDevice {
    /// Creates an independent device for export work.
    ///
    /// The preview surface shares its device with the UI thread, whose frame
    /// pacing can stall background submissions. Export owns this device
    /// outright, so its throughput never depends on window state.
    pub(crate) fn create_headless(
        shaders: &CompiledPluginShaders,
    ) -> Result<Arc<Self>, RenderError> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::PRIMARY,
            flags: wgpu::InstanceFlags::default(),
            memory_budget_thresholds: wgpu::MemoryBudgetThresholds::default(),
            backend_options: wgpu::BackendOptions::default(),
            display: None,
        });
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            force_fallback_adapter: false,
            compatible_surface: None,
            apply_limit_buckets: false,
        }))
        .map_err(|error| {
            RenderError::backend(format!("export GPU adapter is unavailable: {error}"))
        })?;
        let gpu = GpuContext::request(
            &adapter,
            &wgpu::DeviceDescriptor {
                label: Some("zerium-export-device"),
                required_limits: wgpu::Limits::default(),
                ..Default::default()
            },
        )
        .map_err(|error| {
            RenderError::backend(format!("export GPU device is unavailable: {error}"))
        })?;
        RenderDeviceBuilder::new(gpu)?
            .register_plugins(shaders)
            .map(|builder| builder.build())
    }
}
