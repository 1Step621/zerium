//! Build shared bind layouts and built-in rendering pipelines.

use self::{
    interface::{GpuComposite, GpuImage, GpuItem, GpuPass, GpuSurface},
    shaders::{
        ComputePipeline, RasterPipeline, compute_pipeline, effect_pipeline, item_pipeline,
        raster_pipeline,
    },
    video::{VideoPlanePipeline, YuvUploadPipeline},
};
use super::{
    OUTPUT_FORMAT, PROPERTY_WORD_SIZE, SCENE_FORMAT,
    session::{RenderSession, RenderState},
};
use crate::engine::{GpuContext, processing::visual::RenderError};
use std::{
    collections::{BTreeMap, HashMap},
    num::NonZeroU64,
    sync::{Arc, Mutex},
};
use zerium_shader::{
    EffectShaderId, IMAGE_SAMPLER_BINDING, ImageBinding, ItemShaderId, ValidatedEffectShader,
    ValidatedPluginShaders, WgslModule,
};

pub(super) mod interface;
pub(super) mod shaders;
pub(super) mod video;

const COMPOSITE: &str = include_str!("wgsl/composite.wgsl");

impl RenderDevice {
    pub(crate) fn is_lost(&self) -> bool {
        self.gpu.is_lost()
    }

    pub(crate) fn create(
        gpu: GpuContext,
        shaders: &ValidatedPluginShaders,
    ) -> Result<Arc<Self>, RenderError> {
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
        let compute_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("zerium-compute-pipeline-layout"),
                bind_group_layouts: &[
                    Some(&compute_bind_group_layout),
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
        let composite = WgslModule::parse("zerium.composite", COMPOSITE)?;
        for (id, fragment_entry) in [
            ("zerium.composite", "fragment_main"),
            ("zerium.output_transform", "output_fragment_main"),
            ("zerium.blend", "blend_fragment_main"),
        ] {
            composite.validate_render(id, "vertex_main", fragment_entry, &BTreeMap::new())?;
        }
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
        let frame_source = include_str!("wgsl/frame.wgsl");
        WgslModule::parse("zerium.frame_image", frame_source)?.validate_render(
            "zerium.frame_image",
            "vertex_main",
            "fragment_main",
            &BTreeMap::new(),
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
        let yuv_upload = YuvUploadPipeline::new(device)?;
        let video_planes = VideoPlanePipeline::new(device)?;
        let item_pipelines = shaders
            .items()
            .iter()
            .map(|descriptor| {
                Ok((
                    descriptor.id.clone(),
                    item_pipeline(&gpu, &pipeline_layout, descriptor)?,
                ))
            })
            .collect::<Result<_, RenderError>>()?;
        let mut effect_pipelines = HashMap::new();
        let mut compute_pipelines = HashMap::new();
        for effect in shaders.effects() {
            match effect {
                ValidatedEffectShader::Render(descriptor) => {
                    effect_pipelines.insert(
                        descriptor.id.clone(),
                        effect_pipeline(&gpu, &pass_pipeline_layout, descriptor)?,
                    );
                }
                ValidatedEffectShader::Compute(descriptor) => {
                    compute_pipelines.insert(
                        descriptor.id.clone(),
                        compute_pipeline(&gpu, &compute_pipeline_layout, descriptor)?,
                    );
                }
            }
        }
        Ok(Arc::new(Self {
            gpu,
            item_pipelines,
            item_bind_group_layout,
            capability_bind_group_layout,
            pass_bind_group_layout,
            compute_bind_group_layout,
            composite_bind_group_layout,
            backdrop_bind_group_layout,
            effect_pipelines,
            compute_pipelines,
            composite_pipeline,
            blend_pipeline,
            output_pipeline,
            frame_pipeline,
            frame_bind_group_layout,
            sampler,
            yuv_upload,
            video_planes,
        }))
    }

    /// Creates an independent device for export work.
    ///
    /// The preview surface shares its device with the UI thread, whose frame
    /// pacing can stall background submissions. Export owns this device
    /// outright, so its throughput never depends on window state.
    pub(crate) fn create_headless(
        shaders: &ValidatedPluginShaders,
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
        Self::create(gpu, shaders)
    }
}

/// Immutable GPU state. A single device can cheaply create independent render
/// sessions for preview, export, thumbnails, and background jobs.
pub(crate) struct RenderDevice {
    pub(super) gpu: GpuContext,
    pub(super) item_pipelines: HashMap<ItemShaderId, RasterPipeline>,
    pub(super) item_bind_group_layout: wgpu::BindGroupLayout,
    pub(super) capability_bind_group_layout: wgpu::BindGroupLayout,
    pub(super) pass_bind_group_layout: wgpu::BindGroupLayout,
    pub(super) compute_bind_group_layout: wgpu::BindGroupLayout,
    pub(super) composite_bind_group_layout: wgpu::BindGroupLayout,
    pub(super) backdrop_bind_group_layout: wgpu::BindGroupLayout,
    pub(super) effect_pipelines: HashMap<EffectShaderId, RasterPipeline>,
    pub(super) compute_pipelines: HashMap<EffectShaderId, ComputePipeline>,
    pub(super) composite_pipeline: wgpu::RenderPipeline,
    pub(super) blend_pipeline: wgpu::RenderPipeline,
    pub(super) output_pipeline: wgpu::RenderPipeline,
    pub(super) frame_pipeline: wgpu::RenderPipeline,
    pub(super) frame_bind_group_layout: wgpu::BindGroupLayout,
    pub(super) sampler: wgpu::Sampler,
    pub(super) yuv_upload: YuvUploadPipeline,
    pub(super) video_planes: VideoPlanePipeline,
}

impl RenderDevice {
    pub(crate) fn create_session(self: &Arc<Self>) -> RenderSession {
        RenderSession {
            shared: self.clone(),
            state: Mutex::new(RenderState::default()),
        }
    }
}
