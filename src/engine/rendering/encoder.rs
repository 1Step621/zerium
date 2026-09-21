use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RenderOutput {
    EffectA,
    EffectB,
    Composition(usize),
    Temporal { depth: usize, is_a: bool },
    Cached(usize),
}

impl RenderOutput {
    fn view(self, resources: &RenderResources) -> Result<&wgpu::TextureView, RenderError> {
        match self {
            Self::EffectA => Ok(&resources.effect_view_a),
            Self::EffectB => Ok(&resources.effect_view_b),
            Self::Composition(depth) => resources
                .compositions
                .get(depth)
                .map(|resource| &resource.view)
                .ok_or_else(|| RenderError::backend("composition render resource is missing")),
            Self::Temporal { depth, is_a } => resources
                .temporal
                .get(depth)
                .map(|resource| {
                    if is_a {
                        &resource.view_a
                    } else {
                        &resource.view_b
                    }
                })
                .ok_or_else(|| RenderError::backend("temporal render resource is missing")),
            Self::Cached(slot) => resources
                .cached_nodes
                .get(slot)
                .map(|resource| &resource.view)
                .ok_or_else(|| RenderError::backend("shared render node resource is missing")),
        }
    }

    fn texture(self, resources: &RenderResources) -> Result<&wgpu::Texture, RenderError> {
        match self {
            Self::EffectA => Ok(&resources.effect_texture_a),
            Self::EffectB => Ok(&resources.effect_texture_b),
            Self::Composition(depth) => resources
                .compositions
                .get(depth)
                .map(|resource| &resource.texture)
                .ok_or_else(|| RenderError::backend("composition render resource is missing")),
            Self::Temporal { depth, is_a } => resources
                .temporal
                .get(depth)
                .map(|resource| {
                    if is_a {
                        &resource.texture_a
                    } else {
                        &resource.texture_b
                    }
                })
                .ok_or_else(|| RenderError::backend("temporal render resource is missing")),
            Self::Cached(slot) => resources
                .cached_nodes
                .get(slot)
                .map(|resource| &resource.texture)
                .ok_or_else(|| RenderError::backend("shared render node resource is missing")),
        }
    }

    fn next_effect_target(self) -> Self {
        if self == Self::EffectA {
            Self::EffectB
        } else {
            Self::EffectA
        }
    }
}

#[derive(Clone, Copy)]
struct RenderDepth {
    temporal: usize,
    composition: usize,
}

struct RenderNodeContext<'a> {
    resources: &'a RenderResources,
    textures: &'a [TextureResource],
    nodes: &'a [RenderNodeCommand],
    shared_node_slots: &'a [Option<usize>],
    stride: u32,
}

impl RenderDepth {
    const ROOT: Self = Self {
        temporal: 0,
        composition: 0,
    };
}

impl FrameRenderer {
    pub(super) fn encode_item_pass(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        target_view: &wgpu::TextureView,
        bind_group: &wgpu::BindGroup,
        shader: &ItemShaderId,
        instances: Range<u32>,
        load: wgpu::LoadOp<wgpu::Color>,
    ) {
        let color_attachments = [Some(wgpu::RenderPassColorAttachment {
            view: target_view,
            depth_slice: None,
            resolve_target: None,
            ops: wgpu::Operations {
                load,
                store: wgpu::StoreOp::Store,
            },
        })];
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("zerium-item-pass"),
            color_attachments: &color_attachments,
            ..Default::default()
        });
        let pipeline = self
            .pipelines
            .get(shader)
            .expect("scene shaders were validated before encoding");
        pass.set_pipeline(&pipeline.pipeline);
        pass.set_bind_group(0, bind_group, &[]);
        pass.draw(0..pipeline.vertex_count, instances);
    }

    pub(super) fn encode_texture_pass(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        target_view: &wgpu::TextureView,
        bind_group: &wgpu::BindGroup,
        shader: &TextureShaderId,
        load: wgpu::LoadOp<wgpu::Color>,
    ) {
        let color_attachments = [Some(wgpu::RenderPassColorAttachment {
            view: target_view,
            depth_slice: None,
            resolve_target: None,
            ops: wgpu::Operations {
                load,
                store: wgpu::StoreOp::Store,
            },
        })];
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("zerium-video-pass"),
            color_attachments: &color_attachments,
            ..Default::default()
        });
        let pipeline = self
            .texture_pipelines
            .get(shader)
            .expect("scene texture item shaders were validated before encoding");
        pass.set_pipeline(&pipeline.pipeline);
        pass.set_bind_group(0, bind_group, &[]);
        pass.draw(0..pipeline.vertex_count, 0..1);
    }

    fn rendered_texture_bind_group(
        &self,
        texture: &TextureResource,
        input: &wgpu::TextureView,
        shader: &TextureShaderId,
    ) -> Result<wgpu::BindGroup, RenderError> {
        if texture.input_count != 1 || !matches!(&texture.binding, TextureBinding::Rendered) {
            return Err(RenderError::backend(
                "rendered texture resource has an invalid input shape",
            ));
        }
        let pipeline = self
            .texture_pipelines
            .get(shader)
            .ok_or_else(|| RenderError::backend("rendered texture shader is not registered"))?;
        Ok(self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("zerium-rendered-texture-bind-group"),
            layout: &pipeline.bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: texture._item.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: texture._item_properties.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(input),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: texture._input_properties.as_entire_binding(),
                },
            ],
        }))
    }

    fn effect_input_bind_group(
        &self,
        resources: &RenderResources,
        input: &wgpu::TextureView,
    ) -> wgpu::BindGroup {
        self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("zerium-dynamic-effect-input"),
            layout: &self.effect_bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(input),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                        buffer: &resources.effect_instance_buffer,
                        offset: 0,
                        size: NonZeroU64::new(size_of::<GpuEffect>() as u64),
                    }),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: resources.effect_property_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: wgpu::BindingResource::TextureView(&resources.effect_source_view),
                },
            ],
        })
    }

    fn composite_input_bind_group(
        &self,
        input: &wgpu::TextureView,
        info: &wgpu::Buffer,
    ) -> wgpu::BindGroup {
        self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("zerium-dynamic-composite-input"),
            layout: &self.composite_bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(input),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: info.as_entire_binding(),
                },
            ],
        })
    }

    fn compute_input_bind_group(
        &self,
        resources: &RenderResources,
        input: &wgpu::TextureView,
        output: &wgpu::TextureView,
    ) -> wgpu::BindGroup {
        self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("zerium-dynamic-compute-input"),
            layout: &self.compute_bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(input),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                        buffer: &resources.compute_info_buffer,
                        offset: 0,
                        size: NonZeroU64::new(size_of::<GpuCompute>() as u64),
                    }),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: resources.effect_property_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: wgpu::BindingResource::TextureView(output),
                },
                wgpu::BindGroupEntry {
                    binding: 5,
                    resource: wgpu::BindingResource::TextureView(&resources.effect_source_view),
                },
            ],
        })
    }

    fn temporal_input_bind_group(
        &self,
        resources: &RenderResources,
        sample: &wgpu::TextureView,
        accumulation: &wgpu::TextureView,
    ) -> wgpu::BindGroup {
        self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("zerium-dynamic-temporal-input"),
            layout: &self.temporal_bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(sample),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(accumulation),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                        buffer: &resources.effect_instance_buffer,
                        offset: 0,
                        size: NonZeroU64::new(size_of::<GpuEffect>() as u64),
                    }),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: resources.effect_property_buffer.as_entire_binding(),
                },
            ],
        })
    }

    pub(super) fn encode_effect_pass(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        target_view: &wgpu::TextureView,
        input: &wgpu::BindGroup,
        shader: &EffectShaderId,
        instance_offset: u32,
    ) {
        let color_attachments = [Some(wgpu::RenderPassColorAttachment {
            view: target_view,
            depth_slice: None,
            resolve_target: None,
            ops: wgpu::Operations {
                load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                store: wgpu::StoreOp::Store,
            },
        })];
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("zerium-effect-pass"),
            color_attachments: &color_attachments,
            ..Default::default()
        });
        let pipeline = self
            .effect_pipelines
            .get(shader)
            .expect("scene effect shaders were validated before encoding");
        pass.set_pipeline(&pipeline.pipeline);
        pass.set_bind_group(0, input, &[instance_offset]);
        pass.draw(0..pipeline.vertex_count, 0..1);
    }

    pub(super) fn encode_composite_pass(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        target_view: &wgpu::TextureView,
        input: &wgpu::BindGroup,
    ) {
        let color_attachments = [Some(wgpu::RenderPassColorAttachment {
            view: target_view,
            depth_slice: None,
            resolve_target: None,
            ops: wgpu::Operations {
                load: wgpu::LoadOp::Load,
                store: wgpu::StoreOp::Store,
            },
        })];
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("zerium-effect-composite-pass"),
            color_attachments: &color_attachments,
            ..Default::default()
        });
        pass.set_pipeline(&self.composite_pipeline);
        pass.set_bind_group(0, input, &[]);
        pass.draw(0..3, 0..1);
    }

    fn encode_output_pass(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        target_view: &wgpu::TextureView,
        input: &wgpu::BindGroup,
    ) {
        let color_attachments = [Some(wgpu::RenderPassColorAttachment {
            view: target_view,
            depth_slice: None,
            resolve_target: None,
            ops: wgpu::Operations {
                load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                store: wgpu::StoreOp::Store,
            },
        })];
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("zerium-output-transform-pass"),
            color_attachments: &color_attachments,
            ..Default::default()
        });
        pass.set_pipeline(&self.output_pipeline);
        pass.set_bind_group(0, input, &[]);
        pass.draw(0..3, 0..1);
    }

    fn encode_temporal_reduce_pass(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        target_view: &wgpu::TextureView,
        input: &wgpu::BindGroup,
        command: &TemporalReduceCommand,
        instance_offset: u32,
    ) {
        let color_attachments = [Some(wgpu::RenderPassColorAttachment {
            view: target_view,
            depth_slice: None,
            resolve_target: None,
            ops: wgpu::Operations {
                load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                store: wgpu::StoreOp::Store,
            },
        })];
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("zerium-temporal-reduce-pass"),
            color_attachments: &color_attachments,
            ..Default::default()
        });
        let pipeline = self
            .temporal_pipelines
            .get(&command.reducer)
            .expect("temporal shaders were validated before encoding");
        pass.set_pipeline(&pipeline.pipeline);
        pass.set_bind_group(0, input, &[instance_offset]);
        pass.draw(0..pipeline.vertex_count, 0..1);
    }

    pub(super) fn encode_compute_pass(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        resources: &RenderResources,
        input: &wgpu::BindGroup,
        pass_command: &EffectPassCommand,
    ) -> Result<(), RenderError> {
        let EffectPassCommandKind::Compute { dispatch } = pass_command.kind else {
            return Err(RenderError::backend(
                "render pass was sent to the compute encoder",
            ));
        };
        let pipeline = self
            .compute_pipelines
            .get(&pass_command.shader)
            .ok_or_else(|| {
                RenderError::backend(format!(
                    "compute effect shader '{}' is not registered",
                    pass_command.shader
                ))
            })?;
        let instance_offset = u64::from(pass_command.instance)
            .checked_mul(resources.compute_info_stride)
            .and_then(|offset| u32::try_from(offset).ok())
            .ok_or_else(|| RenderError::backend("compute info offset exceeds u32"))?;
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("zerium-compute-pass"),
            timestamp_writes: None,
        });
        pass.set_pipeline(&pipeline.pipeline);
        pass.set_bind_group(0, input, &[instance_offset]);
        let extent = |dimension: ComputeDispatchDimension| match dimension {
            ComputeDispatchDimension::Width => resources.size.width,
            ComputeDispatchDimension::Height => resources.size.height,
            ComputeDispatchDimension::MaxDimension => {
                resources.size.width.max(resources.size.height)
            }
            ComputeDispatchDimension::One => 1,
        };
        pass.dispatch_workgroups(
            extent(dispatch[0]).div_ceil(pipeline.workgroup_size[0]),
            extent(dispatch[1]).div_ceil(pipeline.workgroup_size[1]),
            extent(dispatch[2]).div_ceil(pipeline.workgroup_size[2]),
        );
        Ok(())
    }

    fn encode_effect_passes(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        resources: &RenderResources,
        passes: &[EffectPassCommand],
        stride: u32,
        mut output: RenderOutput,
    ) -> Result<RenderOutput, RenderError> {
        for effect_pass in passes {
            if effect_pass.captures_source {
                encoder.copy_texture_to_texture(
                    wgpu::TexelCopyTextureInfo {
                        texture: output.texture(resources)?,
                        mip_level: 0,
                        origin: wgpu::Origin3d::ZERO,
                        aspect: wgpu::TextureAspect::All,
                    },
                    wgpu::TexelCopyTextureInfo {
                        texture: &resources.effect_source_texture,
                        mip_level: 0,
                        origin: wgpu::Origin3d::ZERO,
                        aspect: wgpu::TextureAspect::All,
                    },
                    wgpu::Extent3d {
                        width: resources.size.width,
                        height: resources.size.height,
                        depth_or_array_layers: 1,
                    },
                );
            }
            let instance_offset = effect_pass
                .instance
                .checked_mul(stride)
                .ok_or_else(|| RenderError::backend("effect instance offset exceeds u32"))?;
            let target = output.next_effect_target();
            let input_view = output.view(resources)?;
            let target_view = target.view(resources)?;
            let dynamic_effect_input;
            let effect_input = match output {
                RenderOutput::EffectA => &resources.effect_input_a,
                RenderOutput::EffectB => &resources.effect_input_b,
                RenderOutput::Composition(_)
                | RenderOutput::Temporal { .. }
                | RenderOutput::Cached(_) => {
                    dynamic_effect_input = self.effect_input_bind_group(resources, input_view);
                    &dynamic_effect_input
                }
            };
            match effect_pass.kind {
                EffectPassCommandKind::Render => self.encode_effect_pass(
                    encoder,
                    target_view,
                    effect_input,
                    &effect_pass.shader,
                    instance_offset,
                ),
                EffectPassCommandKind::Compute { .. } => {
                    let dynamic_compute_input;
                    let compute_input = match (output, target) {
                        (RenderOutput::EffectA, RenderOutput::EffectB) => {
                            &resources.compute_inputs[0]
                        }
                        (RenderOutput::EffectB, RenderOutput::EffectA) => {
                            &resources.compute_inputs[1]
                        }
                        _ => {
                            dynamic_compute_input =
                                self.compute_input_bind_group(resources, input_view, target_view);
                            &dynamic_compute_input
                        }
                    };
                    self.encode_compute_pass(encoder, resources, compute_input, effect_pass)?
                }
            }
            output = target;
        }
        Ok(output)
    }

    fn encode_render_node(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        context: &RenderNodeContext<'_>,
        rendered_shared_nodes: &mut [bool],
        node_id: RenderNodeId,
        depth: RenderDepth,
    ) -> Result<RenderOutput, RenderError> {
        if let Some(slot) = context.shared_node_slots[node_id] {
            if rendered_shared_nodes[slot] {
                return Ok(RenderOutput::Cached(slot));
            }
            let output = self.encode_render_node_uncached(
                encoder,
                context,
                rendered_shared_nodes,
                node_id,
                depth,
            )?;
            let cached = RenderOutput::Cached(slot);
            encoder.copy_texture_to_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: output.texture(context.resources)?,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                wgpu::TexelCopyTextureInfo {
                    texture: cached.texture(context.resources)?,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                wgpu::Extent3d {
                    width: context.resources.size.width,
                    height: context.resources.size.height,
                    depth_or_array_layers: 1,
                },
            );
            rendered_shared_nodes[slot] = true;
            return Ok(cached);
        }
        self.encode_render_node_uncached(encoder, context, rendered_shared_nodes, node_id, depth)
    }

    fn encode_render_node_uncached(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        context: &RenderNodeContext<'_>,
        rendered_shared_nodes: &mut [bool],
        node_id: RenderNodeId,
        depth: RenderDepth,
    ) -> Result<RenderOutput, RenderError> {
        let resources = context.resources;
        let textures = context.textures;
        let nodes = context.nodes;
        let stride = context.stride;
        let temporal_depth = depth.temporal;
        let composition_depth = depth.composition;
        let node = &nodes[node_id];
        match &node.kind {
            RenderNodeCommandKind::Source(source) => {
                match source {
                    RenderSourceCommand::Transparent => {
                        let color_attachments = [Some(wgpu::RenderPassColorAttachment {
                            view: &resources.effect_view_a,
                            depth_slice: None,
                            resolve_target: None,
                            ops: wgpu::Operations {
                                load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                                store: wgpu::StoreOp::Store,
                            },
                        })];
                        encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                            label: Some("zerium-transparent-temporal-sample"),
                            color_attachments: &color_attachments,
                            ..Default::default()
                        });
                    }
                    RenderSourceCommand::Item { shader, instance } => self.encode_item_pass(
                        encoder,
                        &resources.effect_view_a,
                        &resources.bind_group,
                        shader,
                        *instance..*instance + 1,
                        wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                    ),
                    RenderSourceCommand::Texture { index, shader } => {
                        let texture = textures.get(*index).ok_or_else(|| {
                            RenderError::backend(
                                "temporal render source references a missing texture",
                            )
                        })?;
                        let TextureBinding::Static(bind_group) = &texture.binding else {
                            return Err(RenderError::backend(
                                "decoded texture source has no static bind group",
                            ));
                        };
                        self.encode_texture_pass(
                            encoder,
                            &resources.effect_view_a,
                            bind_group,
                            shader,
                            wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        );
                    }
                    RenderSourceCommand::RenderedTexture {
                        input,
                        index,
                        shader,
                    } => {
                        let input = self.encode_render_node(
                            encoder,
                            context,
                            rendered_shared_nodes,
                            *input,
                            depth,
                        )?;
                        let target = input.next_effect_target();
                        let input_view = input.view(resources)?;
                        let target_view = target.view(resources)?;
                        let texture = textures.get(*index).ok_or_else(|| {
                            RenderError::backend(
                                "rendered source references a missing texture resource",
                            )
                        })?;
                        let bind_group =
                            self.rendered_texture_bind_group(texture, input_view, shader)?;
                        self.encode_texture_pass(
                            encoder,
                            target_view,
                            &bind_group,
                            shader,
                            wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        );
                        return Ok(target);
                    }
                }
                Ok(RenderOutput::EffectA)
            }
            RenderNodeCommandKind::Composite { children } => {
                let composition =
                    resources
                        .compositions
                        .get(composition_depth)
                        .ok_or_else(|| {
                            RenderError::backend("scene composition resource depth is insufficient")
                        })?;
                let attachments = [Some(wgpu::RenderPassColorAttachment {
                    view: &composition.view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                })];
                encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("zerium-scene-node-clear"),
                    color_attachments: &attachments,
                    ..Default::default()
                });
                for child in children {
                    match &nodes[*child].kind {
                        RenderNodeCommandKind::Source(RenderSourceCommand::Transparent) => {}
                        RenderNodeCommandKind::Source(RenderSourceCommand::Item {
                            shader,
                            instance,
                        }) => self.encode_item_pass(
                            encoder,
                            &composition.view,
                            &resources.bind_group,
                            shader,
                            *instance..*instance + 1,
                            wgpu::LoadOp::Load,
                        ),
                        RenderNodeCommandKind::Source(RenderSourceCommand::Texture {
                            index,
                            shader,
                        }) => {
                            let texture = textures.get(*index).ok_or_else(|| {
                                RenderError::backend(
                                    "scene node references a missing texture input",
                                )
                            })?;
                            let TextureBinding::Static(bind_group) = &texture.binding else {
                                return Err(RenderError::backend(
                                    "decoded texture source has no static bind group",
                                ));
                            };
                            self.encode_texture_pass(
                                encoder,
                                &composition.view,
                                bind_group,
                                shader,
                                wgpu::LoadOp::Load,
                            );
                        }
                        RenderNodeCommandKind::Source(RenderSourceCommand::RenderedTexture {
                            ..
                        })
                        | RenderNodeCommandKind::Composite { .. }
                        | RenderNodeCommandKind::Effect { .. }
                        | RenderNodeCommandKind::TemporalEffect { .. } => {
                            let child_output = self.encode_render_node(
                                encoder,
                                context,
                                rendered_shared_nodes,
                                *child,
                                RenderDepth {
                                    temporal: temporal_depth,
                                    composition: composition_depth + 1,
                                },
                            )?;
                            let dynamic_input;
                            let input = match child_output {
                                RenderOutput::EffectA => &resources.composition_input_a,
                                RenderOutput::EffectB => &resources.composition_input_b,
                                RenderOutput::Composition(_)
                                | RenderOutput::Temporal { .. }
                                | RenderOutput::Cached(_) => {
                                    dynamic_input = self.composite_input_bind_group(
                                        child_output.view(resources)?,
                                        &resources._composition_info_buffer,
                                    );
                                    &dynamic_input
                                }
                            };
                            self.encode_composite_pass(encoder, &composition.view, input);
                        }
                    }
                }
                Ok(RenderOutput::Composition(composition_depth))
            }
            RenderNodeCommandKind::Effect { input, passes } => {
                let input = self.encode_render_node(
                    encoder,
                    context,
                    rendered_shared_nodes,
                    *input,
                    depth,
                )?;
                self.encode_effect_passes(encoder, resources, passes, stride, input)
            }
            RenderNodeCommandKind::TemporalEffect { samples } => {
                let temporal = resources.temporal.get(temporal_depth).ok_or_else(|| {
                    RenderError::backend("temporal render resource depth is insufficient")
                })?;
                let clear_attachments = [Some(wgpu::RenderPassColorAttachment {
                    view: &temporal.view_a,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                })];
                encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("zerium-temporal-accumulation-clear"),
                    color_attachments: &clear_attachments,
                    ..Default::default()
                });
                let mut accumulation_is_a = true;
                for (sample, reduce) in samples {
                    let sample_output = self.encode_render_node(
                        encoder,
                        context,
                        rendered_shared_nodes,
                        *sample,
                        RenderDepth {
                            temporal: temporal_depth + 1,
                            composition: composition_depth,
                        },
                    )?;
                    let target_view = if accumulation_is_a {
                        &temporal.view_b
                    } else {
                        &temporal.view_a
                    };
                    let accumulation_view = if accumulation_is_a {
                        &temporal.view_a
                    } else {
                        &temporal.view_b
                    };
                    let instance_offset = reduce.instance.checked_mul(stride).ok_or_else(|| {
                        RenderError::backend("temporal instance offset exceeds u32")
                    })?;
                    let dynamic_input;
                    let input = match sample_output {
                        RenderOutput::EffectA | RenderOutput::EffectB => {
                            let sample_is_a = sample_output == RenderOutput::EffectA;
                            let input_index =
                                usize::from(!sample_is_a) * 2 + usize::from(!accumulation_is_a);
                            &temporal.inputs[input_index]
                        }
                        RenderOutput::Composition(_)
                        | RenderOutput::Temporal { .. }
                        | RenderOutput::Cached(_) => {
                            dynamic_input = self.temporal_input_bind_group(
                                resources,
                                sample_output.view(resources)?,
                                accumulation_view,
                            );
                            &dynamic_input
                        }
                    };
                    self.encode_temporal_reduce_pass(
                        encoder,
                        target_view,
                        input,
                        reduce,
                        instance_offset,
                    );
                    accumulation_is_a = !accumulation_is_a;
                }
                Ok(RenderOutput::Temporal {
                    depth: temporal_depth,
                    is_a: accumulation_is_a,
                })
            }
        }
    }

    pub(super) fn encode_scene(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        target_view: &wgpu::TextureView,
        resources_by_scale: &HashMap<u32, RenderResources>,
        textures: &[TextureResource],
        scene: &EncodedScene,
        background: [f64; 4],
    ) -> Result<Vec<(u32, usize, Arc<RenderNodeKey>)>, RenderError> {
        let decode = |value: f64| {
            if value <= 0.04045 {
                value / 12.92
            } else {
                ((value + 0.055) / 1.055).powf(2.4)
            }
        };
        let background = wgpu::Color {
            r: decode(background[0]),
            g: decode(background[1]),
            b: decode(background[2]),
            a: background[3],
        };
        {
            let color_attachments = [Some(wgpu::RenderPassColorAttachment {
                view: target_view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(background),
                    store: wgpu::StoreOp::Store,
                },
            })];
            encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("zerium-frame-clear-pass"),
                color_attachments: &color_attachments,
                ..Default::default()
            });
        }

        let stride = u32::try_from(
            resources_by_scale
                .get(&1)
                .expect("scale-one render resources are always created")
                .effect_instance_stride,
        )
        .map_err(|_| RenderError::backend("effect instance stride exceeds u32"))?;
        let shared_node_count = scene.shared_node_slots.iter().flatten().count();
        let mut keys_by_slot = vec![None; shared_node_count];
        for (node, slot) in scene.shared_node_slots.iter().enumerate() {
            if let Some(slot) = slot {
                keys_by_slot[*slot] = Some(scene.node_keys[node].clone());
            }
        }
        let mut rendered_by_scale = HashMap::<u32, Vec<bool>>::new();
        for command in &scene.commands {
            match command {
                RenderCommand::Items(batch) => {
                    let base_resources = resources_by_scale
                        .get(&1)
                        .expect("scale-one render resources are always created");
                    self.encode_item_pass(
                        encoder,
                        target_view,
                        &base_resources.bind_group,
                        &batch.shader,
                        batch.instances.clone(),
                        wgpu::LoadOp::Load,
                    );
                }
                RenderCommand::Texture { index, shader } => {
                    let texture = textures.get(*index).ok_or_else(|| {
                        RenderError::backend("video render command references a missing frame")
                    })?;
                    let TextureBinding::Static(bind_group) = &texture.binding else {
                        return Err(RenderError::backend(
                            "decoded texture source has no static bind group",
                        ));
                    };
                    self.encode_texture_pass(
                        encoder,
                        target_view,
                        bind_group,
                        shader,
                        wgpu::LoadOp::Load,
                    );
                }
                RenderCommand::Effected { node, render_scale } => {
                    let resources = resources_by_scale
                        .get(render_scale)
                        .expect("effect render resources were created for the command scale");
                    let rendered_shared_nodes =
                        rendered_by_scale.entry(*render_scale).or_insert_with(|| {
                            resources
                                .cached_node_keys
                                .iter()
                                .zip(&keys_by_slot)
                                .map(|(cached, expected)| cached == expected && expected.is_some())
                                .collect()
                        });
                    let output = {
                        let context = RenderNodeContext {
                            resources,
                            textures,
                            nodes: &scene.nodes,
                            shared_node_slots: &scene.shared_node_slots,
                            stride,
                        };
                        self.encode_render_node(
                            encoder,
                            &context,
                            rendered_shared_nodes,
                            *node,
                            RenderDepth::ROOT,
                        )?
                    };
                    let dynamic_input;
                    let composite_input = match output {
                        RenderOutput::EffectA => &resources.composite_input_a,
                        RenderOutput::EffectB => &resources.composite_input_b,
                        RenderOutput::Composition(_)
                        | RenderOutput::Temporal { .. }
                        | RenderOutput::Cached(_) => {
                            dynamic_input = self.composite_input_bind_group(
                                output.view(resources)?,
                                &resources._composite_info_buffer,
                            );
                            &dynamic_input
                        }
                    };
                    self.encode_composite_pass(encoder, target_view, composite_input);
                }
            }
        }
        Ok(rendered_by_scale
            .into_iter()
            .flat_map(|(scale, rendered)| {
                keys_by_slot
                    .iter()
                    .zip(rendered)
                    .enumerate()
                    .filter(|(_, (_, rendered))| *rendered)
                    .map(move |(slot, (key, _))| {
                        (scale, slot, key.clone().expect("slots have keys"))
                    })
            })
            .collect())
    }

    pub(crate) fn render_to_view(
        &self,
        scene: &RenderScene,
        target_view: &wgpu::TextureView,
    ) -> Result<wgpu::SubmissionIndex, RenderError> {
        self.validate_scene(scene)?;
        let encoded = encode_items(scene)?;
        let effect_pass_count = encoded.effects.len();
        let temporal_depth = encoded
            .commands
            .iter()
            .filter_map(|command| match command {
                RenderCommand::Effected { node, .. } => {
                    Some(encoded.nodes[*node].temporal_depth(&encoded.nodes))
                }
                RenderCommand::Items(_) | RenderCommand::Texture { .. } => None,
            })
            .max()
            .unwrap_or(0);
        let composition_depth = encoded
            .commands
            .iter()
            .filter_map(|command| match command {
                RenderCommand::Effected { node, .. } => {
                    Some(encoded.nodes[*node].composition_depth(&encoded.nodes))
                }
                RenderCommand::Items(_) | RenderCommand::Texture { .. } => None,
            })
            .max()
            .unwrap_or(0);
        let shared_node_count = encoded.shared_node_slots.iter().flatten().count();
        let texture_resources = self.create_texture_resources(&encoded.textures)?;
        let mut resources = self
            .resources
            .lock()
            .map_err(|_| RenderError::backend("render resource lock poisoned"))?;
        let mut required_scales = HashSet::from([1_u32]);
        for command in &encoded.commands {
            match command {
                RenderCommand::Effected { render_scale, .. } => {
                    required_scales.insert(*render_scale);
                }
                RenderCommand::Items(_) | RenderCommand::Texture { .. } => {}
            }
        }
        resources.retain(|scale, resources| {
            required_scales.contains(scale)
                && resources.output_size == scene.size
                && resources.composition_size == scene.composition_size
        });
        for scale in &required_scales {
            let effect_size = scene.size.checked_scale(*scale).ok_or_else(|| {
                RenderError::backend(format!("effect render scale {scale} overflows"))
            })?;
            let rebuild = resources.get(scale).is_none_or(|resources| {
                resources.size != effect_size
                    || resources.item_capacity < encoded.items.len().max(1)
                    || resources.property_capacity
                        < encoded.properties.len().max(PROPERTY_WORD_SIZE)
                    || resources.effect_instance_capacity < effect_pass_count.max(1)
                    || resources.effect_property_capacity
                        < encoded.effect_properties.len().max(PROPERTY_WORD_SIZE)
                    || resources.compositions.len() < composition_depth
                    || resources.temporal.len() < temporal_depth
                    || resources.cached_nodes.len() < shared_node_count
            });
            if rebuild {
                resources.insert(
                    *scale,
                    self.create_resources(
                        effect_size,
                        scene.size,
                        scene.composition_size,
                        RenderResourceRequirements {
                            item_count: encoded.items.len(),
                            property_size: encoded.properties.len(),
                            effect_pass_count,
                            effect_property_size: encoded.effect_properties.len(),
                            composition_depth,
                            temporal_depth,
                            shared_node_count,
                        },
                    )?,
                );
            }
        }

        let stride = usize::try_from(
            resources
                .get(&1)
                .expect("scale-one resources were created")
                .effect_instance_stride,
        )
        .map_err(|_| RenderError::backend("effect instance stride exceeds usize"))?;
        let instances =
            (effect_pass_count > 0).then(|| Self::effect_instance_data(&encoded.effects, stride));
        for resources in resources.values() {
            if !encoded.items.is_empty() {
                self.queue.write_buffer(
                    &resources.item_buffer,
                    0,
                    bytemuck::cast_slice(&encoded.items),
                );
            }
            if !encoded.properties.is_empty() {
                self.queue
                    .write_buffer(&resources.property_buffer, 0, &encoded.properties);
            }
            if let Some(instances) = &instances {
                self.queue
                    .write_buffer(&resources.effect_instance_buffer, 0, instances);
                let compute_stride = usize::try_from(resources.compute_info_stride)
                    .map_err(|_| RenderError::backend("compute info stride exceeds usize"))?;
                let compute_instances = Self::compute_instance_data(
                    &encoded.effects,
                    compute_stride,
                    resources.size,
                    resources.composition_size,
                );
                self.queue
                    .write_buffer(&resources.compute_info_buffer, 0, &compute_instances);
                if !encoded.effect_properties.is_empty() {
                    self.queue.write_buffer(
                        &resources.effect_property_buffer,
                        0,
                        &encoded.effect_properties,
                    );
                }
            }
        }

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("zerium-preview-frame-encoder"),
            });
        let scene_view = resources
            .get(&1)
            .expect("scale-one render resources were created")
            .scene_view
            .clone();
        let cache_updates = self.encode_scene(
            &mut encoder,
            &scene_view,
            &resources,
            &texture_resources,
            &encoded,
            scene.background,
        )?;
        let base_resources = resources
            .get(&1)
            .expect("scale-one render resources were created");
        self.encode_output_pass(&mut encoder, target_view, &base_resources.output_input);
        let submission = self.queue.submit([encoder.finish()]);
        for (scale, slot, key) in cache_updates {
            if let Some(resources) = resources.get_mut(&scale) {
                resources.cached_node_keys[slot] = Some(key);
            }
        }
        // The queue retains the submitted work, so scratch buffers are parked
        // for the next frame instead of destroyed.
        let mut cache = self
            .video_textures
            .lock()
            .map_err(|_| RenderError::backend("video texture cache lock poisoned"))?;
        for resource in texture_resources {
            cache.scratch.push(ScratchTextureBuffers {
                input_count: resource.input_count,
                property_size: usize::try_from(resource._item_properties.size()).map_err(|_| {
                    RenderError::backend("texture item property size exceeds usize")
                })?,
                input_properties: resource._input_properties,
                item_properties: resource._item_properties,
                item: resource._item,
            });
        }
        Ok(submission)
    }
}
