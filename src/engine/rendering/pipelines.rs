use super::*;

impl FrameRenderer {
    pub(crate) fn new(
        device: Arc<wgpu::Device>,
        queue: Arc<wgpu::Queue>,
        plugins: &PluginRegistry,
    ) -> Result<Self, RenderError> {
        validate_plugin_shaders(plugins)?;
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
                            min_binding_size: NonZeroU64::new(PARAM_WORD_SIZE as u64),
                        },
                        count: None,
                    },
                ],
            });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("zerium-frame-pipeline-layout"),
            bind_group_layouts: &[Some(&item_bind_group_layout)],
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
                            min_binding_size: NonZeroU64::new(PARAM_WORD_SIZE as u64),
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
                            min_binding_size: NonZeroU64::new(PARAM_WORD_SIZE as u64),
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
                            min_binding_size: NonZeroU64::new(PARAM_WORD_SIZE as u64),
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
                bind_group_layouts: &[Some(&effect_bind_group_layout)],
                immediate_size: 0,
            });
        let temporal_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("zerium-temporal-pipeline-layout"),
                bind_group_layouts: &[Some(&temporal_bind_group_layout)],
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
        let mut renderer = Self {
            shared: Arc::new(RendererDevice {
                device,
                queue,
                pipeline_layout,
                pipelines: HashMap::new(),
                item_bind_group_layout,
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
            }),
            resources: Mutex::new(HashMap::new()),
            video_textures: Mutex::new(VideoTextureCache::default()),
        };
        for descriptor in Self::plugin_item_shaders(plugins)? {
            renderer.register_item_shader(descriptor)?;
        }
        for (plugin_id, schema) in plugins.effects() {
            for (pass_index, pass) in schema.passes().iter().enumerate() {
                let id = EffectShaderId::new(format!(
                    "{plugin_id}::effect::{}::pass::{pass_index}",
                    schema.id()
                ));
                let shader_source = pass.shader_source();
                let source = plugins
                    .shader_source(plugin_id, shader_source)
                    .ok_or_else(|| {
                        RenderError::backend(format!(
                            "plugin '{}' effect shader source '{}' was not loaded",
                            plugin_id, shader_source
                        ))
                    })?;
                match pass {
                    EffectPassSchema::Render { .. } => {
                        let descriptor = EffectShaderDescriptor::from_schema(
                            schema,
                            pass,
                            id,
                            format!("{} pass {pass_index}", schema.id()),
                            source.to_owned(),
                        )?;
                        renderer.register_effect_shader(descriptor)?;
                    }
                    EffectPassSchema::Compute { shader, .. } => {
                        renderer.register_compute_shader(schema, pass, id, shader, source)?;
                    }
                    EffectPassSchema::Temporal { reducer, .. } => {
                        renderer.register_temporal_shader(schema, pass, id, reducer, source)?;
                    }
                }
            }
        }
        for (plugin_id, schema, source) in Self::plugin_texture_shaders(plugins)? {
            renderer.register_texture_shader(plugin_id, schema, source)?;
        }
        Ok(renderer)
    }

    pub(super) fn register_compute_shader(
        &mut self,
        schema: &EffectSchema,
        pass: &EffectPassSchema,
        id: EffectShaderId,
        shader: &crate::domain::plugin::ComputeShaderSchema,
        wgsl: &str,
    ) -> Result<(), RenderError> {
        if self.compute_pipelines.contains_key(&id) {
            return Err(RenderError::backend(format!(
                "compute effect shader '{}' is already registered",
                id
            )));
        }
        let parameter_interface = schema
            .wgsl_parameter_interface(pass)
            .map_err(|error| RenderError::backend(error.to_string()))?;
        let source = format!("{COMPUTE_INTERFACE}\n{parameter_interface}\n{wgsl}");
        let workgroup_size = validate_compute_shader(&id, &source, shader.entry())?;
        let error_scope = self.device.push_error_scope(wgpu::ErrorFilter::Validation);
        let module = self
            .device
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some(id.as_str()),
                source: wgpu::ShaderSource::Wgsl(source.into()),
            });
        let layout = self
            .device
            .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("zerium-compute-pipeline-layout"),
                bind_group_layouts: &[Some(&self.compute_bind_group_layout)],
                immediate_size: 0,
            });
        let pipeline = self
            .device
            .create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some(id.as_str()),
                layout: Some(&layout),
                module: &module,
                entry_point: Some(shader.entry()),
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
        schema: &EffectSchema,
        pass: &EffectPassSchema,
        id: EffectShaderId,
        shader: &crate::domain::plugin::ShaderSchema,
        wgsl: &str,
    ) -> Result<(), RenderError> {
        if self.temporal_pipelines.contains_key(&id) {
            return Err(RenderError::backend(format!(
                "temporal effect reducer '{}' is already registered",
                id
            )));
        }
        let parameter_interface = schema
            .wgsl_parameter_interface(pass)
            .map_err(|error| RenderError::backend(error.to_string()))?;
        let source = format!("{TEMPORAL_INTERFACE}\n{parameter_interface}\n{wgsl}");
        validate_render_shader(&id, &source, shader.vertex_entry(), shader.fragment_entry())?;
        let error_scope = self.device.push_error_scope(wgpu::ErrorFilter::Validation);
        let module = self
            .device
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some(id.as_str()),
                source: wgpu::ShaderSource::Wgsl(source.into()),
            });
        let pipeline = self
            .device
            .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(id.as_str()),
                layout: Some(&self.temporal_pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &module,
                    entry_point: Some(shader.vertex_entry()),
                    compilation_options: Default::default(),
                    buffers: &[],
                },
                primitive: Default::default(),
                depth_stencil: None,
                multisample: Default::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &module,
                    entry_point: Some(shader.fragment_entry()),
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
            EffectPipeline {
                pipeline,
                vertex_count: 3,
            },
        );
        Ok(())
    }

    pub(super) fn plugin_item_shaders(
        plugins: &PluginRegistry,
    ) -> Result<Vec<ItemShaderDescriptor>, RenderError> {
        plugins
            .items()
            .filter_map(|(plugin_id, schema)| {
                if !schema.is_procedural() {
                    return None;
                }
                Some((plugin_id, schema, schema.visual_shader()?))
            })
            .map(|(plugin_id, schema, shader)| {
                let source = plugins
                    .shader_source(plugin_id, shader.source())
                    .ok_or_else(|| {
                        RenderError::backend(format!(
                            "plugin '{}' shader source '{}' was not loaded",
                            plugin_id,
                            shader.source()
                        ))
                    })?;
                ItemShaderDescriptor::from_schema(
                    plugin_id,
                    schema,
                    shader.source().to_owned(),
                    source.to_owned(),
                )
            })
            .collect()
    }

    pub(super) fn plugin_texture_shaders(
        plugins: &PluginRegistry,
    ) -> Result<Vec<(&str, &ItemSchema, &str)>, RenderError> {
        plugins
            .items()
            .filter_map(|(plugin_id, schema)| {
                if !schema.uses_texture_pipeline() {
                    return None;
                }
                Some((plugin_id, schema, schema.visual_shader()?))
            })
            .map(|(plugin_id, schema, shader)| {
                plugins
                    .shader_source(plugin_id, shader.source())
                    .map(|source| (plugin_id, schema, source))
                    .ok_or_else(|| {
                        RenderError::backend(format!(
                            "plugin '{}' texture shader source '{}' was not loaded",
                            plugin_id,
                            shader.source()
                        ))
                    })
            })
            .collect()
    }

    pub(super) fn register_texture_shader(
        &mut self,
        plugin_id: &str,
        schema: &ItemSchema,
        source: &str,
    ) -> Result<(), RenderError> {
        let shader = schema
            .uses_texture_pipeline()
            .then(|| schema.visual_shader())
            .flatten()
            .ok_or_else(|| RenderError::backend("item has no texture visual capability"))?;
        let id = TextureShaderId::new(format!("{plugin_id}::item::{}", schema.id()));
        if self.texture_pipelines.contains_key(&id) {
            return Err(RenderError::backend(format!(
                "texture item shader '{}' is already registered",
                id
            )));
        }
        let parameter_interface = schema
            .wgsl_parameter_interface()
            .map_err(|error| RenderError::backend(error.to_string()))?;
        let input_ids = texture_input_ids(schema);
        let media_interface = texture_media_interface(&input_ids);
        let source =
            format!("{ITEM_INTERFACE}\n{parameter_interface}\n{media_interface}\n{source}");
        validate_render_shader(&id, &source, shader.vertex_entry(), shader.fragment_entry())?;
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
                    min_binding_size: NonZeroU64::new(PARAM_WORD_SIZE as u64),
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
                label: Some(id.as_str()),
                source: wgpu::ShaderSource::Wgsl(source.into()),
            });
        let pipeline = self
            .device
            .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(id.as_str()),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &module,
                    entry_point: Some(shader.vertex_entry()),
                    compilation_options: Default::default(),
                    buffers: &[],
                },
                primitive: Default::default(),
                depth_stencil: None,
                multisample: Default::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &module,
                    entry_point: Some(shader.fragment_entry()),
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
                vertex_count: schema
                    .vertex_count()
                    .expect("texture visual capability was checked"),
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

        let source = format!(
            "{ITEM_INTERFACE}\n{}\n{}",
            descriptor.parameter_interface, descriptor.wgsl
        );
        validate_render_shader(
            &descriptor.id,
            &source,
            &descriptor.vertex_entry,
            &descriptor.fragment_entry,
        )?;
        let error_scope = self.device.push_error_scope(wgpu::ErrorFilter::Validation);
        let module = self
            .device
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some(&descriptor.label),
                source: wgpu::ShaderSource::Wgsl(source.into()),
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
            ItemPipeline {
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

        let source = format!(
            "{EFFECT_INTERFACE}\n{}\n{}",
            descriptor.parameter_interface, descriptor.wgsl
        );
        validate_render_shader(
            &descriptor.id,
            &source,
            &descriptor.vertex_entry,
            &descriptor.fragment_entry,
        )?;
        let error_scope = self.device.push_error_scope(wgpu::ErrorFilter::Validation);
        let module = self
            .device
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some(&descriptor.label),
                source: wgpu::ShaderSource::Wgsl(source.into()),
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
            EffectPipeline {
                pipeline,
                vertex_count: descriptor.vertex_count,
            },
        );
        Ok(())
    }
}
