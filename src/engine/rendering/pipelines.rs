use super::shader_compile::validate_render_shader;
use super::*;

impl RendererBuilder {
    pub(crate) fn new(
        device: Arc<wgpu::Device>,
        queue: Arc<wgpu::Queue>,
    ) -> Result<Self, RenderError> {
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
                ],
            });
        let mut capability_entries = (0..capability_input::MAX_INPUTS)
            .map(|binding| wgpu::BindGroupLayoutEntry {
                binding: binding as u32,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT | wgpu::ShaderStages::COMPUTE,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            })
            .collect::<Vec<_>>();
        capability_entries.push(wgpu::BindGroupLayoutEntry {
            binding: capability_input::SAMPLER_BINDING as u32,
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
        let effect_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("zerium-effect-bind-group-layout"),
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
                            has_dynamic_offset: true,
                            min_binding_size: NonZeroU64::new(size_of::<GpuEffect>() as u64),
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 3,
                        visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Storage { read_only: true },
                            has_dynamic_offset: false,
                            min_binding_size: NonZeroU64::new(PROPERTY_WORD_SIZE as u64),
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 4,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Float { filterable: true },
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    },
                ],
            });
        let temporal_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("zerium-temporal-bind-group-layout"),
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
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Float { filterable: true },
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 2,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 3,
                        visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Uniform,
                            has_dynamic_offset: true,
                            min_binding_size: NonZeroU64::new(size_of::<GpuEffect>() as u64),
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 4,
                        visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Storage { read_only: true },
                            has_dynamic_offset: false,
                            min_binding_size: NonZeroU64::new(PROPERTY_WORD_SIZE as u64),
                        },
                        count: None,
                    },
                ],
            });
        let compute_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("zerium-compute-bind-group-layout"),
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::COMPUTE,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Float { filterable: true },
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        visibility: wgpu::ShaderStages::COMPUTE,
                        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 2,
                        visibility: wgpu::ShaderStages::COMPUTE,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Uniform,
                            has_dynamic_offset: true,
                            min_binding_size: NonZeroU64::new(size_of::<GpuCompute>() as u64),
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 3,
                        visibility: wgpu::ShaderStages::COMPUTE,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Storage { read_only: true },
                            has_dynamic_offset: false,
                            min_binding_size: NonZeroU64::new(PROPERTY_WORD_SIZE as u64),
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 4,
                        visibility: wgpu::ShaderStages::COMPUTE,
                        ty: wgpu::BindingType::StorageTexture {
                            access: wgpu::StorageTextureAccess::WriteOnly,
                            format: SCENE_FORMAT,
                            view_dimension: wgpu::TextureViewDimension::D2,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 5,
                        visibility: wgpu::ShaderStages::COMPUTE,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Float { filterable: true },
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    },
                ],
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
        let effect_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("zerium-effect-pipeline-layout"),
                bind_group_layouts: &[
                    Some(&effect_bind_group_layout),
                    Some(&capability_bind_group_layout),
                ],
                immediate_size: 0,
            });
        let temporal_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("zerium-temporal-pipeline-layout"),
                bind_group_layouts: &[
                    Some(&temporal_bind_group_layout),
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
        let composite_module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("zerium-composite-shader"),
            source: wgpu::ShaderSource::Wgsl(COMPOSITE.into()),
        });
        let premultiplied_blend = wgpu::BlendState {
            color: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::One,
                dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                operation: wgpu::BlendOperation::Add,
            },
            alpha: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::One,
                dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                operation: wgpu::BlendOperation::Add,
            },
        };
        let composite_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("zerium-composite-pipeline"),
            layout: Some(&composite_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &composite_module,
                entry_point: Some("vertex_main"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState {
                module: &composite_module,
                entry_point: Some("fragment_main"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: SCENE_FORMAT,
                    blend: Some(premultiplied_blend),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        let output_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("zerium-output-transform-pipeline"),
            layout: Some(&composite_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &composite_module,
                entry_point: Some("vertex_main"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState {
                module: &composite_module,
                entry_point: Some("output_fragment_main"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: OUTPUT_FORMAT,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("zerium-effect-sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        Ok(Self {
            device: RendererDevice {
                device,
                queue,
                pipeline_layout,
                pipelines: HashMap::new(),
                item_bind_group_layout,
                capability_bind_group_layout,
                effect_bind_group_layout,
                temporal_bind_group_layout,
                compute_bind_group_layout,
                effect_pipeline_layout,
                temporal_pipeline_layout,
                composite_bind_group_layout,
                effect_pipelines: HashMap::new(),
                temporal_pipelines: HashMap::new(),
                compute_pipelines: HashMap::new(),
                composite_pipeline,
                output_pipeline,
                texture_pipelines: HashMap::new(),
                sampler,
            },
        })
    }

    pub(crate) fn register_plugins(
        mut self,
        shaders: &CompiledPluginShaders,
    ) -> Result<Self, RenderError> {
        for descriptor in &shaders.items {
            self.device.register_item_shader(descriptor.clone())?;
        }
        for effect in &shaders.effects {
            match effect {
                CompiledEffectShader::Render(descriptor) => {
                    self.device.register_effect_shader(descriptor.clone())?;
                }
                CompiledEffectShader::Compute(descriptor) => {
                    self.device.register_compute_shader(descriptor.clone())?;
                }
                CompiledEffectShader::Temporal(descriptor) => {
                    self.device.register_temporal_shader(descriptor.clone())?;
                }
            }
        }
        for descriptor in &shaders.textures {
            self.device.register_texture_shader(descriptor.clone())?;
        }
        Ok(self)
    }

    pub(crate) fn build(self) -> Arc<RendererDevice> {
        Arc::new(self.device)
    }
}

impl RendererDevice {
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
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("zerium-export-device"),
            required_features: wgpu::Features::empty(),
            required_limits: wgpu::Limits::default(),
            ..Default::default()
        }))
        .map_err(|error| {
            RenderError::backend(format!("export GPU device is unavailable: {error}"))
        })?;
        RendererBuilder::new(Arc::new(device), Arc::new(queue))?
            .register_plugins(shaders)
            .map(|builder| builder.build())
    }

    pub(super) fn register_compute_shader(
        &mut self,
        descriptor: ComputeShaderDescriptor,
    ) -> Result<(), RenderError> {
        let ComputeShaderDescriptor {
            id,
            wgsl,
            entry,
            workgroup_size,
        } = descriptor;
        if self.compute_pipelines.contains_key(&id) {
            return Err(RenderError::backend(format!(
                "compute effect shader '{}' is already registered",
                id
            )));
        }
        let error_scope = self.device.push_error_scope(wgpu::ErrorFilter::Validation);
        let module = self
            .device
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some(id.as_str()),
                source: wgpu::ShaderSource::Wgsl(wgsl.as_ref().into()),
            });
        let layout = self
            .device
            .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("zerium-compute-pipeline-layout"),
                bind_group_layouts: &[
                    Some(&self.compute_bind_group_layout),
                    Some(&self.capability_bind_group_layout),
                ],
                immediate_size: 0,
            });
        let pipeline = self
            .device
            .create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some(id.as_str()),
                layout: Some(&layout),
                module: &module,
                entry_point: Some(&entry),
                compilation_options: Default::default(),
                cache: None,
            });
        if let Some(error) = pollster::block_on(error_scope.pop()) {
            return Err(RenderError::backend(format!(
                "compute effect shader '{}' is incompatible with the render pipeline: {error}",
                id
            )));
        }
        self.compute_pipelines.insert(
            id,
            ComputePipeline {
                pipeline,
                workgroup_size,
            },
        );
        Ok(())
    }

    pub(super) fn register_temporal_shader(
        &mut self,
        descriptor: EffectShaderDescriptor,
    ) -> Result<(), RenderError> {
        let EffectShaderDescriptor {
            id,
            label,
            wgsl,
            vertex_entry,
            fragment_entry,
        } = descriptor;
        if self.temporal_pipelines.contains_key(&id) {
            return Err(RenderError::backend(format!(
                "temporal effect reducer '{}' is already registered",
                id
            )));
        }
        let error_scope = self.device.push_error_scope(wgpu::ErrorFilter::Validation);
        let module = self
            .device
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some(&label),
                source: wgpu::ShaderSource::Wgsl(wgsl.as_ref().into()),
            });
        let pipeline = self
            .device
            .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(&label),
                layout: Some(&self.temporal_pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &module,
                    entry_point: Some(&vertex_entry),
                    compilation_options: Default::default(),
                    buffers: &[],
                },
                primitive: Default::default(),
                depth_stencil: None,
                multisample: Default::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &module,
                    entry_point: Some(&fragment_entry),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: SCENE_FORMAT,
                        blend: None,
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                multiview_mask: None,
                cache: None,
            });
        if let Some(error) = pollster::block_on(error_scope.pop()) {
            return Err(RenderError::backend(format!(
                "temporal effect reducer '{}' is incompatible with the render pipeline: {error}",
                id
            )));
        }
        self.temporal_pipelines.insert(
            id,
            RasterPipeline {
                pipeline,
                vertex_count: 3,
            },
        );
        Ok(())
    }

    pub(super) fn register_texture_shader(
        &mut self,
        descriptor: TextureShaderDescriptor,
    ) -> Result<(), RenderError> {
        let TextureShaderDescriptor {
            id,
            label,
            wgsl,
            vertex_entry,
            fragment_entry,
            vertex_count,
            input_ids,
        } = descriptor;
        if self.texture_pipelines.contains_key(&id) {
            return Err(RenderError::backend(format!(
                "texture item shader '{}' is already registered",
                id
            )));
        }
        let error_scope = self.device.push_error_scope(wgpu::ErrorFilter::Validation);
        let sampler_binding = u32::try_from(2 + input_ids.len())
            .map_err(|_| RenderError::backend("too many texture inputs"))?;
        let metadata_binding = sampler_binding + 1;
        let mut layout_entries = vec![
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
        ];
        layout_entries.extend(
            (0..input_ids.len()).map(|index| wgpu::BindGroupLayoutEntry {
                binding: u32::try_from(index + 2).expect("texture binding index exceeds u32"),
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            }),
        );
        layout_entries.push(wgpu::BindGroupLayoutEntry {
            binding: sampler_binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
            count: None,
        });
        layout_entries.push(wgpu::BindGroupLayoutEntry {
            binding: metadata_binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: NonZeroU64::new(((input_ids.len() + 1) * 16) as u64),
            },
            count: None,
        });
        let bind_group_layout =
            self.device
                .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                    label: Some("zerium-texture-item-bind-group-layout"),
                    entries: &layout_entries,
                });
        let pipeline_layout = self
            .device
            .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("zerium-texture-item-pipeline-layout"),
                bind_group_layouts: &[Some(&bind_group_layout)],
                immediate_size: 0,
            });
        let module = self
            .device
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some(&label),
                source: wgpu::ShaderSource::Wgsl(wgsl.as_ref().into()),
            });
        let pipeline = self
            .device
            .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(&label),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &module,
                    entry_point: Some(&vertex_entry),
                    compilation_options: Default::default(),
                    buffers: &[],
                },
                primitive: Default::default(),
                depth_stencil: None,
                multisample: Default::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &module,
                    entry_point: Some(&fragment_entry),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: SCENE_FORMAT,
                        blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                multiview_mask: None,
                cache: None,
            });
        if let Some(error) = pollster::block_on(error_scope.pop()) {
            return Err(RenderError::backend(format!(
                "texture item shader '{}' is incompatible with the render pipeline: {error}",
                id
            )));
        }
        self.texture_pipelines.insert(
            id,
            TexturePipeline {
                pipeline,
                vertex_count,
                bind_group_layout,
                input_ids,
            },
        );
        Ok(())
    }

    /// Registers a kind-specific WGSL fragment. This is the extension point for
    /// future plugins; IDs must be unique for the lifetime of the renderer.
    pub(super) fn register_item_shader(
        &mut self,
        descriptor: ItemShaderDescriptor,
    ) -> Result<(), RenderError> {
        if descriptor.id.as_str().is_empty() {
            return Err(RenderError::backend("item shader ID must not be empty"));
        }
        if self.pipelines.contains_key(&descriptor.id) {
            return Err(RenderError::backend(format!(
                "item shader '{}' is already registered",
                descriptor.id
            )));
        }

        let error_scope = self.device.push_error_scope(wgpu::ErrorFilter::Validation);
        let module = self
            .device
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some(&descriptor.label),
                source: wgpu::ShaderSource::Wgsl(descriptor.wgsl.as_ref().into()),
            });
        let pipeline = self
            .device
            .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(&descriptor.label),
                layout: Some(&self.pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &module,
                    entry_point: Some(&descriptor.vertex_entry),
                    compilation_options: Default::default(),
                    buffers: &[],
                },
                primitive: Default::default(),
                depth_stencil: None,
                multisample: Default::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &module,
                    entry_point: Some(&descriptor.fragment_entry),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: SCENE_FORMAT,
                        blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                multiview_mask: None,
                cache: None,
            });
        if let Some(error) = pollster::block_on(error_scope.pop()) {
            return Err(RenderError::backend(format!(
                "item shader '{}' is incompatible with the render pipeline: {error}",
                descriptor.id
            )));
        }
        self.pipelines.insert(
            descriptor.id,
            RasterPipeline {
                pipeline,
                vertex_count: descriptor.vertex_count,
            },
        );
        Ok(())
    }

    pub(super) fn register_effect_shader(
        &mut self,
        descriptor: EffectShaderDescriptor,
    ) -> Result<(), RenderError> {
        if descriptor.id.as_str().is_empty() {
            return Err(RenderError::backend("effect shader ID must not be empty"));
        }
        if self.effect_pipelines.contains_key(&descriptor.id) {
            return Err(RenderError::backend(format!(
                "effect shader '{}' is already registered",
                descriptor.id
            )));
        }

        let error_scope = self.device.push_error_scope(wgpu::ErrorFilter::Validation);
        let module = self
            .device
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some(&descriptor.label),
                source: wgpu::ShaderSource::Wgsl(descriptor.wgsl.as_ref().into()),
            });
        let pipeline = self
            .device
            .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(&descriptor.label),
                layout: Some(&self.effect_pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &module,
                    entry_point: Some(&descriptor.vertex_entry),
                    compilation_options: Default::default(),
                    buffers: &[],
                },
                primitive: Default::default(),
                depth_stencil: None,
                multisample: Default::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &module,
                    entry_point: Some(&descriptor.fragment_entry),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: SCENE_FORMAT,
                        blend: None,
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                multiview_mask: None,
                cache: None,
            });
        if let Some(error) = pollster::block_on(error_scope.pop()) {
            return Err(RenderError::backend(format!(
                "effect shader '{}' is incompatible with the render pipeline: {error}",
                descriptor.id
            )));
        }
        self.effect_pipelines.insert(
            descriptor.id,
            RasterPipeline {
                pipeline,
                vertex_count: 3,
            },
        );
        Ok(())
    }
}
