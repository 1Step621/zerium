use super::*;

impl FrameRenderer {
    pub(super) fn create_resources(
        &self,
        size: RenderSize,
        output_size: RenderSize,
        composition_size: RenderSize,
        requirements: RenderResourceRequirements,
    ) -> Result<RenderResources, RenderError> {
        let RenderResourceRequirements {
            item_count,
            params_size,
            effect_pass_count,
            effect_params_size,
            composition_depth,
            temporal_depth,
        } = requirements;
        let item_capacity = item_count
            .max(1)
            .checked_next_power_of_two()
            .ok_or_else(|| RenderError::backend("item buffer capacity overflow"))?;
        let params_capacity = params_size
            .max(PARAM_WORD_SIZE)
            .checked_next_power_of_two()
            .ok_or_else(|| RenderError::backend("parameter buffer capacity overflow"))?;
        let item_buffer_size = (item_capacity as u64)
            .checked_mul(size_of::<GpuItem>() as u64)
            .ok_or_else(|| RenderError::backend("item buffer size overflow"))?;
        let params_buffer_size = params_capacity as u64;
        let limits = self.device.limits();
        let effect_instance_stride = u64::from(limits.min_uniform_buffer_offset_alignment)
            .max(size_of::<GpuEffect>() as u64)
            .next_multiple_of(u64::from(limits.min_uniform_buffer_offset_alignment).max(1));
        let effect_instance_capacity = effect_pass_count
            .max(1)
            .checked_next_power_of_two()
            .ok_or_else(|| RenderError::backend("effect instance capacity overflow"))?;
        let effect_instance_buffer_size = effect_instance_stride
            .checked_mul(effect_instance_capacity as u64)
            .ok_or_else(|| RenderError::backend("effect instance buffer size overflow"))?;
        let effect_params_capacity = effect_params_size
            .max(PARAM_WORD_SIZE)
            .checked_next_power_of_two()
            .ok_or_else(|| RenderError::backend("effect parameter buffer capacity overflow"))?;
        let effect_params_buffer_size = effect_params_capacity as u64;
        let compute_info_stride = u64::from(limits.min_uniform_buffer_offset_alignment)
            .max(size_of::<GpuCompute>() as u64)
            .next_multiple_of(u64::from(limits.min_uniform_buffer_offset_alignment).max(1));
        let compute_info_buffer_size = compute_info_stride
            .checked_mul(effect_instance_capacity as u64)
            .ok_or_else(|| RenderError::backend("compute info buffer size overflow"))?;
        for (name, size) in [
            ("item", item_buffer_size),
            ("parameter", params_buffer_size),
            ("effect parameter", effect_params_buffer_size),
        ] {
            if size > limits.max_buffer_size || size > limits.max_storage_buffer_binding_size {
                return Err(RenderError::backend(format!(
                    "{name} buffer size {size} exceeds GPU limits"
                )));
            }
        }

        let item_buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("zerium-item-buffer"),
            size: item_buffer_size,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let params_buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("zerium-item-params-buffer"),
            size: params_buffer_size,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("zerium-item-bind-group"),
            layout: &self.item_bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: item_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: params_buffer.as_entire_binding(),
                },
            ],
        });

        let effect_instance_buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("zerium-effect-instance-buffer"),
            size: effect_instance_buffer_size,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let effect_params_buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("zerium-effect-params-buffer"),
            size: effect_params_buffer_size,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let compute_info_buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("zerium-compute-info-buffer"),
            size: compute_info_buffer_size,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let composite_info_buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("zerium-composite-info"),
            size: size_of::<GpuComposite>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        self.queue.write_buffer(
            &composite_info_buffer,
            0,
            bytemuck::bytes_of(&GpuComposite {
                input_size: [size.width, size.height],
                output_size: [output_size.width, output_size.height],
            }),
        );
        let scene_texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("zerium-scene-linear-texture"),
            size: wgpu::Extent3d {
                width: output_size.width,
                height: output_size.height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: SCENE_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let scene_view = scene_texture.create_view(&Default::default());
        let effect_texture_descriptor = wgpu::TextureDescriptor {
            label: Some("zerium-effect-texture"),
            size: wgpu::Extent3d {
                width: size.width,
                height: size.height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: SCENE_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::STORAGE_BINDING
                | wgpu::TextureUsages::COPY_DST
                | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        };
        let effect_texture_a = self.device.create_texture(&effect_texture_descriptor);
        let effect_texture_descriptor = wgpu::TextureDescriptor {
            label: Some("zerium-effect-texture-b"),
            ..effect_texture_descriptor
        };
        let effect_texture_b = self.device.create_texture(&effect_texture_descriptor);
        let effect_source_descriptor = wgpu::TextureDescriptor {
            label: Some("zerium-effect-source-texture"),
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            ..effect_texture_descriptor
        };
        let effect_source_texture = self.device.create_texture(&effect_source_descriptor);
        let effect_view_a = effect_texture_a.create_view(&Default::default());
        let effect_view_b = effect_texture_b.create_view(&Default::default());
        let effect_source_view = effect_source_texture.create_view(&Default::default());
        let effect_input = |label, view: &wgpu::TextureView| {
            self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some(label),
                layout: &self.effect_bind_group_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(&self.sampler),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                            buffer: &effect_instance_buffer,
                            offset: 0,
                            size: NonZeroU64::new(size_of::<GpuEffect>() as u64),
                        }),
                    },
                    wgpu::BindGroupEntry {
                        binding: 3,
                        resource: effect_params_buffer.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 4,
                        resource: wgpu::BindingResource::TextureView(&effect_source_view),
                    },
                ],
            })
        };
        let effect_input_a = effect_input("zerium-effect-input-a", &effect_view_a);
        let effect_input_b = effect_input("zerium-effect-input-b", &effect_view_b);
        let compute_input =
            |label, input_view: &wgpu::TextureView, output_view: &wgpu::TextureView| {
                self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some(label),
                    layout: &self.compute_bind_group_layout,
                    entries: &[
                        wgpu::BindGroupEntry {
                            binding: 0,
                            resource: wgpu::BindingResource::TextureView(input_view),
                        },
                        wgpu::BindGroupEntry {
                            binding: 1,
                            resource: wgpu::BindingResource::Sampler(&self.sampler),
                        },
                        wgpu::BindGroupEntry {
                            binding: 2,
                            resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                                buffer: &compute_info_buffer,
                                offset: 0,
                                size: NonZeroU64::new(size_of::<GpuCompute>() as u64),
                            }),
                        },
                        wgpu::BindGroupEntry {
                            binding: 3,
                            resource: effect_params_buffer.as_entire_binding(),
                        },
                        wgpu::BindGroupEntry {
                            binding: 4,
                            resource: wgpu::BindingResource::TextureView(output_view),
                        },
                        wgpu::BindGroupEntry {
                            binding: 5,
                            resource: wgpu::BindingResource::TextureView(&effect_source_view),
                        },
                    ],
                })
            };
        let compute_inputs = [
            compute_input(
                "zerium-compute-input-a-output-b",
                &effect_view_a,
                &effect_view_b,
            ),
            compute_input(
                "zerium-compute-input-b-output-a",
                &effect_view_b,
                &effect_view_a,
            ),
        ];
        let composite_input = |label, view: &wgpu::TextureView| {
            self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some(label),
                layout: &self.composite_bind_group_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(&self.sampler),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: composite_info_buffer.as_entire_binding(),
                    },
                ],
            })
        };
        let composite_input_a = composite_input("zerium-composite-input-a", &effect_view_a);
        let composite_input_b = composite_input("zerium-composite-input-b", &effect_view_b);
        let output_input = composite_input("zerium-output-input", &scene_view);
        let compositions = (0..composition_depth)
            .map(|_| {
                let descriptor = wgpu::TextureDescriptor {
                    label: Some("zerium-scene-node-composition"),
                    usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                        | wgpu::TextureUsages::TEXTURE_BINDING
                        | wgpu::TextureUsages::COPY_SRC,
                    ..effect_texture_descriptor
                };
                let texture = self.device.create_texture(&descriptor);
                let view = texture.create_view(&Default::default());
                CompositionRenderResource { texture, view }
            })
            .collect();
        let temporal = (0..temporal_depth)
            .map(|_| {
                let descriptor = wgpu::TextureDescriptor {
                    label: Some("zerium-temporal-accumulation-texture"),
                    usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                        | wgpu::TextureUsages::TEXTURE_BINDING
                        | wgpu::TextureUsages::COPY_SRC,
                    ..effect_texture_descriptor
                };
                let texture_a = self.device.create_texture(&descriptor);
                let texture_b = self.device.create_texture(&descriptor);
                let view_a = texture_a.create_view(&Default::default());
                let view_b = texture_b.create_view(&Default::default());
                let mut inputs = Vec::with_capacity(4);
                for sample_view in [&effect_view_a, &effect_view_b] {
                    for accumulation_view in [&view_a, &view_b] {
                        inputs.push(self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                            label: Some("zerium-temporal-input"),
                            layout: &self.temporal_bind_group_layout,
                            entries: &[
                                wgpu::BindGroupEntry {
                                    binding: 0,
                                    resource: wgpu::BindingResource::TextureView(sample_view),
                                },
                                wgpu::BindGroupEntry {
                                    binding: 1,
                                    resource: wgpu::BindingResource::TextureView(accumulation_view),
                                },
                                wgpu::BindGroupEntry {
                                    binding: 2,
                                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                                },
                                wgpu::BindGroupEntry {
                                    binding: 3,
                                    resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                                        buffer: &effect_instance_buffer,
                                        offset: 0,
                                        size: NonZeroU64::new(size_of::<GpuEffect>() as u64),
                                    }),
                                },
                                wgpu::BindGroupEntry {
                                    binding: 4,
                                    resource: effect_params_buffer.as_entire_binding(),
                                },
                            ],
                        }));
                    }
                }
                TemporalRenderResource {
                    texture_a,
                    texture_b,
                    view_a,
                    view_b,
                    inputs,
                }
            })
            .collect();

        Ok(RenderResources {
            size,
            output_size,
            composition_size,
            item_capacity,
            params_capacity,
            item_buffer,
            params_buffer,
            bind_group,
            effect_instance_stride,
            effect_instance_capacity,
            effect_params_capacity,
            effect_instance_buffer,
            effect_params_buffer,
            compute_info_stride,
            compute_info_buffer,
            compute_inputs,
            _composite_info_buffer: composite_info_buffer,
            scene_view,
            output_input,
            effect_texture_a,
            effect_texture_b,
            effect_source_texture,
            effect_view_a,
            effect_view_b,
            effect_input_a,
            effect_input_b,
            composite_input_a,
            composite_input_b,
            compositions,
            temporal,
        })
    }

    pub(super) fn validate_scene(&self, scene: &RenderScene) -> Result<(), RenderError> {
        if scene.size.width == 0 || scene.size.height == 0 {
            return Err(RenderError::backend(
                "render width and height must both be non-zero",
            ));
        }
        let render_items = scene.render_items();
        let render_effects = scene.render_effects();
        let item_count = render_items
            .iter()
            .filter(|item| matches!(item, RenderItem::Shader(_)))
            .count();
        if item_count > u32::MAX as usize {
            return Err(RenderError::backend("too many visible items in one frame"));
        }
        if let Some(item) = render_items
            .iter()
            .filter_map(|item| match item {
                RenderItem::Shader(item) => Some(item),
                RenderItem::Texture(_) => None,
            })
            .find(|item| !self.pipelines.contains_key(&item.shader))
        {
            return Err(RenderError::backend(format!(
                "item shader '{}' is not registered",
                item.shader
            )));
        }
        for pass in render_effects.iter().flat_map(|effect| &effect.passes) {
            let (shader, registered) = match pass {
                RenderEffectPass::Render { shader, .. } => {
                    (shader, self.effect_pipelines.contains_key(shader))
                }
                RenderEffectPass::Compute { shader, .. } => {
                    (shader, self.compute_pipelines.contains_key(shader))
                }
                RenderEffectPass::Temporal { reducer, .. } => {
                    (reducer, self.temporal_pipelines.contains_key(reducer))
                }
            };
            if !registered {
                return Err(RenderError::backend(format!(
                    "effect shader '{}' is not registered",
                    shader
                )));
            }
        }
        if let Some(video) = render_items.iter().find_map(|item| match item {
            RenderItem::Texture(video) if !self.texture_pipelines.contains_key(&video.shader) => {
                Some(video)
            }
            _ => None,
        }) {
            return Err(RenderError::backend(format!(
                "texture item shader '{}' is not registered",
                video.shader
            )));
        }
        let limits = self.device.limits();
        if scene.effect_size.width > limits.max_texture_dimension_2d
            || scene.effect_size.height > limits.max_texture_dimension_2d
        {
            return Err(RenderError::backend(format!(
                "effect render size {}x{} exceeds GPU limit {}",
                scene.effect_size.width, scene.effect_size.height, limits.max_texture_dimension_2d
            )));
        }
        let scale_x = scene.effect_size.width / scene.size.width;
        let scale_y = scene.effect_size.height / scene.size.height;
        if scale_x == 0
            || scale_x != scale_y
            || scene.effect_size.width != scene.size.width.saturating_mul(scale_x)
            || scene.effect_size.height != scene.size.height.saturating_mul(scale_y)
        {
            return Err(RenderError::backend(
                "effect render size must be an integer multiple of the output size",
            ));
        }

        if let Some(frame) = render_items.iter().find_map(|item| match item {
            RenderItem::Texture(video) => video.frames.iter().find(|frame| {
                frame.width == 0
                    || frame.height == 0
                    || frame.width > limits.max_texture_dimension_2d
                    || frame.height > limits.max_texture_dimension_2d
            }),
            RenderItem::Shader(_) => None,
        }) {
            return Err(RenderError::backend(format!(
                "video frame size {}x{} exceeds GPU limits",
                frame.width, frame.height
            )));
        }
        Ok(())
    }

    pub(super) fn effect_instance_data(effects: &[GpuEffect], stride: usize) -> Vec<u8> {
        let mut bytes = vec![0; stride.saturating_mul(effects.len())];
        for (index, effect) in effects.iter().enumerate() {
            let offset = index * stride;
            bytes[offset..offset + size_of::<GpuEffect>()]
                .copy_from_slice(bytemuck::bytes_of(effect));
        }
        bytes
    }

    pub(super) fn compute_instance_data(
        effects: &[GpuEffect],
        stride: usize,
        size: RenderSize,
        composition_size: RenderSize,
    ) -> Vec<u8> {
        let mut bytes = vec![0; stride.saturating_mul(effects.len())];
        for (index, effect) in effects.iter().enumerate() {
            let info = GpuCompute {
                params_offset: effect.params_offset,
                params_size: effect.params_size,
                width: size.width,
                height: size.height,
                composition_size: [
                    composition_size.width as f32,
                    composition_size.height as f32,
                ],
                padding: [0; 2],
            };
            let offset = index * stride;
            bytes[offset..offset + size_of::<GpuCompute>()]
                .copy_from_slice(bytemuck::bytes_of(&info));
        }
        bytes
    }

    pub(super) fn create_texture_resources(
        &self,
        textures: &[EncodedTexture],
    ) -> Result<Vec<TextureResource>, RenderError> {
        let mut cache = self
            .video_textures
            .lock()
            .map_err(|_| RenderError::backend("video texture cache lock poisoned"))?;
        let desired_frames = textures
            .iter()
            .flat_map(|texture| texture.frames.iter())
            .collect::<Vec<_>>();
        let mut current_scene = Vec::with_capacity(textures.len());
        let mut resources = Vec::with_capacity(textures.len());
        for encoded in textures {
            let pipeline = self
                .texture_pipelines
                .get(&encoded.shader)
                .ok_or_else(|| RenderError::backend("texture shader is not registered"))?;
            if encoded.frames.len() != pipeline.input_ids.len() {
                return Err(RenderError::backend(format!(
                    "texture input slot count for shader '{}' does not match its plugin schema",
                    encoded.shader
                )));
            }
            let mut uploaded_frames = Vec::with_capacity(encoded.frames.len());
            for frame in &encoded.frames {
                let uploaded = cache
                    .previous_scene
                    .iter()
                    .chain(&current_scene)
                    .find(|uploaded| Arc::ptr_eq(&uploaded.frame, frame))
                    .cloned()
                    .map(Ok)
                    .unwrap_or_else(|| {
                        let reusable = cache.previous_scene.iter().position(|uploaded| {
                            uploaded.frame.width == frame.width
                                && uploaded.frame.height == frame.height
                                && Arc::strong_count(uploaded) == 1
                                && !desired_frames
                                    .iter()
                                    .any(|desired| Arc::ptr_eq(&uploaded.frame, desired))
                        });
                        let Some(reusable) = reusable else {
                            return self.upload_video_frame(frame.clone());
                        };
                        let mut uploaded = cache.previous_scene.swap_remove(reusable);
                        let slot = Arc::get_mut(&mut uploaded)
                            .expect("reusable video texture must have a single owner");
                        self.update_uploaded_video_frame(slot, frame.clone())?;
                        Ok(uploaded)
                    })?;
                if !current_scene
                    .iter()
                    .any(|current| Arc::ptr_eq(&current.frame, frame))
                {
                    current_scene.push(uploaded.clone());
                }
                uploaded_frames.push(uploaded);
            }
            let mut input_metadata = uploaded_frames
                .iter()
                .map(|uploaded| GpuTextureInput {
                    size: [
                        uploaded.frame.width as f32,
                        uploaded.frame.height as f32,
                        0.,
                        0.,
                    ],
                })
                .collect::<Vec<_>>();
            input_metadata.push(GpuTextureInput {
                size: [
                    encoded.target_size.width as f32,
                    encoded.target_size.height as f32,
                    0.,
                    0.,
                ],
            });
            let input_parameters = self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("zerium-texture-input-parameters"),
                size: (input_metadata.len() * size_of::<GpuTextureInput>()) as u64,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            self.queue
                .write_buffer(&input_parameters, 0, bytemuck::cast_slice(&input_metadata));
            let item_params_size = encoded.params.len().max(PARAM_WORD_SIZE);
            let item_params = self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("zerium-texture-item-params"),
                size: item_params_size as u64,
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            if !encoded.params.as_bytes().is_empty() {
                self.queue
                    .write_buffer(&item_params, 0, encoded.params.as_bytes());
            }
            let item = self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("zerium-texture-item"),
                size: size_of::<GpuItem>() as u64,
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            self.queue.write_buffer(
                &item,
                0,
                bytemuck::bytes_of(&GpuItem {
                    params_offset: 0,
                    params_size: u32::try_from(encoded.params.len()).map_err(|_| {
                        RenderError::backend("texture item parameter size exceeds u32")
                    })?,
                    output_size: [
                        encoded.target_size.width as f32,
                        encoded.target_size.height as f32,
                    ],
                    composition_size: [
                        encoded.composition_size.width as f32,
                        encoded.composition_size.height as f32,
                    ],
                }),
            );
            let mut bind_entries = vec![
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: item.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: item_params.as_entire_binding(),
                },
            ];
            bind_entries.extend(uploaded_frames.iter().enumerate().map(|(index, uploaded)| {
                wgpu::BindGroupEntry {
                    binding: u32::try_from(index + 2).expect("texture binding index exceeds u32"),
                    resource: wgpu::BindingResource::TextureView(&uploaded.view),
                }
            }));
            let sampler_binding = u32::try_from(2 + uploaded_frames.len())
                .map_err(|_| RenderError::backend("too many texture inputs"))?;
            bind_entries.push(wgpu::BindGroupEntry {
                binding: sampler_binding,
                resource: wgpu::BindingResource::Sampler(&self.sampler),
            });
            bind_entries.push(wgpu::BindGroupEntry {
                binding: sampler_binding + 1,
                resource: input_parameters.as_entire_binding(),
            });
            let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("zerium-texture-item-bind-group"),
                layout: &pipeline.bind_group_layout,
                entries: &bind_entries,
            });
            resources.push(TextureResource {
                _uploaded_frames: uploaded_frames,
                _input_parameters: input_parameters,
                _item: item,
                _item_params: item_params,
                bind_group,
            });
        }
        cache.previous_scene = current_scene;
        Ok(resources)
    }

    pub(super) fn update_uploaded_video_frame(
        &self,
        uploaded: &mut UploadedVideoFrame,
        frame: Arc<RgbaFrame>,
    ) -> Result<(), RenderError> {
        if uploaded.frame.width != frame.width || uploaded.frame.height != frame.height {
            return Err(RenderError::backend(
                "cannot reuse a video texture with different dimensions",
            ));
        }
        let extent = Self::video_frame_extent(&frame)?;
        self.write_video_frame(&uploaded._texture, &frame, extent);
        uploaded.frame = frame;
        Ok(())
    }

    pub(super) fn upload_video_frame(
        &self,
        frame: Arc<RgbaFrame>,
    ) -> Result<Arc<UploadedVideoFrame>, RenderError> {
        let extent = Self::video_frame_extent(&frame)?;
        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("zerium-video-frame-texture"),
            size: extent,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: VIDEO_FRAME_FORMAT,
            usage: wgpu::TextureUsages::COPY_DST | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        self.write_video_frame(&texture, &frame, extent);
        let view = texture.create_view(&Default::default());
        Ok(Arc::new(UploadedVideoFrame {
            frame,
            _texture: texture,
            view,
        }))
    }

    pub(super) fn video_frame_extent(frame: &RgbaFrame) -> Result<wgpu::Extent3d, RenderError> {
        let expected_len = usize::try_from(frame.width)
            .ok()
            .and_then(|width| {
                usize::try_from(frame.height)
                    .ok()
                    .and_then(|height| width.checked_mul(height))
            })
            .and_then(|pixels| pixels.checked_mul(4))
            .ok_or_else(|| RenderError::backend("video frame size overflow"))?;
        if frame.rgba.len() != expected_len {
            return Err(RenderError::backend(format!(
                "video frame has {} bytes but {} were expected",
                frame.rgba.len(),
                expected_len
            )));
        }
        Ok(wgpu::Extent3d {
            width: frame.width,
            height: frame.height,
            depth_or_array_layers: 1,
        })
    }

    pub(super) fn write_video_frame(
        &self,
        texture: &wgpu::Texture,
        frame: &RgbaFrame,
        extent: wgpu::Extent3d,
    ) {
        self.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &frame.rgba,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(frame.width.saturating_mul(4)),
                rows_per_image: Some(frame.height),
            },
            extent,
        );
    }
}
