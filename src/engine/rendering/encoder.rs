use super::surface::{BoundsOperation, SurfaceRect};
use super::*;

#[derive(Clone)]
struct RenderedSurface {
    view: wgpu::TextureView,
    rect: SurfaceRect,
    size: RenderSize,
}

struct SceneEncoding {
    cache_updates: Vec<(u32, usize, Arc<RenderNodeKey>)>,
    // Keep transient surfaces alive until their GPU commands are submitted.
    local_resources: Vec<RenderResources>,
    unused_resources: Vec<RenderResources>,
}

const LOCAL_RESOURCE_CACHE_BUDGET: usize = 256 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RenderOutput {
    EffectA,
    EffectB,
}

impl RenderOutput {
    fn view(self, resources: &RenderResources) -> Result<&wgpu::TextureView, RenderError> {
        match self {
            Self::EffectA => Ok(&resources.effect_view_a),
            Self::EffectB => Ok(&resources.effect_view_b),
        }
    }

    fn texture(self, resources: &RenderResources) -> Result<&wgpu::Texture, RenderError> {
        match self {
            Self::EffectA => Ok(&resources.effect_texture_a),
            Self::EffectB => Ok(&resources.effect_texture_b),
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
struct RenderNodeContext<'a> {
    resources: &'a RenderResources,
    encoded: &'a EncodedScene,
    viewport: RenderSize,
    render_scale: u32,
    local_pool: &'a RefCell<Vec<RenderResources>>,
    available_local_resources: &'a RefCell<Vec<RenderResources>>,
    textures: &'a [TextureResource],
    nodes: &'a [RenderNodeCommand],
    shared_node_slots: &'a [Option<usize>],
}

struct EffectPassBindings<'a> {
    input: &'a wgpu::BindGroup,
    capabilities: &'a wgpu::BindGroup,
}

struct EffectPassContext<'a> {
    stride: u32,
    capabilities: &'a [&'a wgpu::TextureView],
}

impl FrameRenderer {
    fn local_resources(
        &self,
        context: &RenderNodeContext<'_>,
        node_id: RenderNodeId,
        bounds: SurfaceRect,
        input_bounds: Option<SurfaceRect>,
    ) -> Result<(RenderResources, SurfaceRect), RenderError> {
        let composition = context.resources.composition_size;
        let (rect, size) = bounds
            .pixel_aligned(context.viewport, composition, context.render_scale)
            .ok_or_else(|| {
                RenderError::resource_limit("render surface bounds cannot be rasterized")
            })?;
        let limit = self.device.limits().max_texture_dimension_2d;
        if size.width > limit || size.height > limit {
            return Err(RenderError::resource_limit(format!(
                "render surface {}x{} exceeds GPU limit {limit}",
                size.width, size.height
            )));
        }
        let encoded = context.encoded;
        let temporal_depth = usize::from(matches!(
            encoded.nodes[node_id].kind,
            RenderNodeCommandKind::TemporalEffect { .. }
        ));
        let requirements = RenderResourceRequirements {
            frame_output: false,
            item_count: encoded.items.len(),
            property_size: encoded.properties.len(),
            effect_pass_count: encoded.effects.len(),
            effect_property_size: encoded.effect_properties.len(),
            temporal_depth,
            shared_node_count: 0,
        };
        let reuse = context
            .available_local_resources
            .borrow()
            .iter()
            .position(|resource| {
                resource.size == size
                    && resource.composition_size == composition
                    && resource.output_size == size
                    && resource.item_capacity >= requirements.item_count.max(1)
                    && resource.property_capacity
                        >= requirements.property_size.max(PROPERTY_WORD_SIZE)
                    && resource.effect_instance_capacity >= requirements.effect_pass_count.max(1)
                    && resource.effect_property_capacity
                        >= requirements.effect_property_size.max(PROPERTY_WORD_SIZE)
                    && resource.temporal.len() >= temporal_depth
            });
        let resources = if let Some(index) = reuse {
            context
                .available_local_resources
                .borrow_mut()
                .swap_remove(index)
        } else {
            self.create_resources(size, size, composition, requirements)?
        };
        let surface_size = [
            (rect.max[0] - rect.min[0]) as f32,
            (rect.max[1] - rect.min[1]) as f32,
        ];
        let surface_min = [rect.min[0] as f32, rect.min[1] as f32];
        if !encoded.items.is_empty() {
            let mut items = encoded.items.clone();
            for item in &mut items {
                item.surface_min = surface_min;
                item.surface_size = surface_size;
                item.output_size = [size.width as f32, size.height as f32];
            }
            self.queue
                .write_buffer(&resources.item_buffer, 0, bytemuck::cast_slice(&items));
        }
        if !encoded.properties.is_empty() {
            self.queue
                .write_buffer(&resources.property_buffer, 0, &encoded.properties);
        }
        if !encoded.effects.is_empty() {
            let mut effects = encoded.effects.clone();
            for effect in &mut effects {
                effect.surface_min = surface_min;
                effect.surface_size = surface_size;
                let input = input_bounds.unwrap_or(rect);
                effect.input_min = [input.min[0] as f32, input.min[1] as f32];
                effect.input_size = [
                    (input.max[0] - input.min[0]) as f32,
                    (input.max[1] - input.min[1]) as f32,
                ];
            }
            let stride = usize::try_from(resources.effect_instance_stride)
                .map_err(|_| RenderError::backend("effect instance stride exceeds usize"))?;
            self.queue.write_buffer(
                &resources.effect_instance_buffer,
                0,
                &Self::effect_instance_data(&effects, stride),
            );
            let compute_stride = usize::try_from(resources.compute_info_stride)
                .map_err(|_| RenderError::backend("compute info stride exceeds usize"))?;
            self.queue.write_buffer(
                &resources.compute_info_buffer,
                0,
                &Self::compute_instance_data(&effects, compute_stride, size, composition),
            );
            if !encoded.effect_properties.is_empty() {
                self.queue.write_buffer(
                    &resources.effect_property_buffer,
                    0,
                    &encoded.effect_properties,
                );
            }
        }
        Ok((resources, rect))
    }

    fn clear_target(&self, encoder: &mut wgpu::CommandEncoder, target: &wgpu::TextureView) {
        let attachments = [Some(wgpu::RenderPassColorAttachment {
            view: target,
            depth_slice: None,
            resolve_target: None,
            ops: wgpu::Operations {
                load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                store: wgpu::StoreOp::Store,
            },
        })];
        encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("zerium-surface-clear"),
            color_attachments: &attachments,
            ..Default::default()
        });
    }

    fn spatial_composite(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        target_rect: SurfaceRect,
        target_size: RenderSize,
        source: &RenderedSurface,
        clear: bool,
    ) {
        if clear {
            self.clear_target(encoder, target);
        }
        let width = target_rect.max[0] - target_rect.min[0];
        let height = target_rect.max[1] - target_rect.min[1];
        let info = GpuComposite {
            input_size: [source.size.width, source.size.height],
            output_size: [target_size.width, target_size.height],
            input_rect: [
                ((source.rect.min[0] - target_rect.min[0]) / width * f64::from(target_size.width))
                    as f32,
                ((source.rect.min[1] - target_rect.min[1]) / height * f64::from(target_size.height))
                    as f32,
                ((source.rect.max[0] - source.rect.min[0]) / width * f64::from(target_size.width))
                    as f32,
                ((source.rect.max[1] - source.rect.min[1]) / height * f64::from(target_size.height))
                    as f32,
            ],
        };
        let buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("zerium-spatial-composite-info"),
            size: size_of::<GpuComposite>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        self.queue
            .write_buffer(&buffer, 0, bytemuck::bytes_of(&info));
        let input = self.composite_input_bind_group(&source.view, &buffer);
        self.encode_composite_pass(encoder, target, &input);
    }

    fn render_surface(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        context: &RenderNodeContext<'_>,
        rendered_shared_nodes: &mut [bool],
        node_id: RenderNodeId,
    ) -> Result<RenderedSurface, RenderError> {
        if let Some(slot) = context.shared_node_slots[node_id] {
            let cached =
                context.resources.cached_nodes.get(slot).ok_or_else(|| {
                    RenderError::backend("shared render node resource is missing")
                })?;
            let rect = SurfaceRect::viewport(context.resources.composition_size);
            if !rendered_shared_nodes[slot] {
                let surface =
                    self.render_surface_uncached(encoder, context, rendered_shared_nodes, node_id)?;
                self.spatial_composite(
                    encoder,
                    &cached.view,
                    rect,
                    context.resources.size,
                    &surface,
                    true,
                );
                rendered_shared_nodes[slot] = true;
            }
            return Ok(RenderedSurface {
                view: cached.view.clone(),
                rect,
                size: context.resources.size,
            });
        }
        self.render_surface_uncached(encoder, context, rendered_shared_nodes, node_id)
    }

    fn render_surface_uncached(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        context: &RenderNodeContext<'_>,
        rendered_shared_nodes: &mut [bool],
        node_id: RenderNodeId,
    ) -> Result<RenderedSurface, RenderError> {
        let node = &context.nodes[node_id];
        match &node.kind {
            RenderNodeCommandKind::Source(source) => {
                let (resources, rect) =
                    self.local_resources(context, node_id, node.bounds, None)?;
                match source {
                    RenderSourceCommand::Transparent => {
                        self.clear_target(encoder, &resources.effect_view_a)
                    }
                    RenderSourceCommand::Item {
                        shader,
                        instance,
                        capabilities,
                    } => {
                        let views = self.render_capability_views(
                            encoder,
                            context,
                            rendered_shared_nodes,
                            capabilities,
                        )?;
                        let inputs = self.capability_bind_group(
                            &views.iter().collect::<Vec<_>>(),
                            &resources.effect_source_view,
                        );
                        self.encode_item_pass(
                            encoder,
                            &resources.effect_view_a,
                            &resources.bind_group,
                            &inputs,
                            shader,
                            *instance..*instance + 1,
                            wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        );
                    }
                    RenderSourceCommand::Texture { index, shader } => {
                        let texture = context.textures.get(*index).ok_or_else(|| {
                            RenderError::backend("item texture resource is missing")
                        })?;
                        self.encode_texture_pass(
                            encoder,
                            &resources.effect_view_a,
                            &texture.binding,
                            shader,
                            wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        );
                    }
                }
                let surface = RenderedSurface {
                    view: resources.effect_view_a.clone(),
                    rect,
                    size: resources.size,
                };
                context.local_pool.borrow_mut().push(resources);
                Ok(surface)
            }
            RenderNodeCommandKind::Effect {
                input,
                capabilities,
                bounds_operation,
                passes,
            } => {
                let input_surface =
                    self.render_surface(encoder, context, rendered_shared_nodes, *input)?;
                if matches!(bounds_operation, BoundsOperation::Translate(_))
                    && node.bounds
                        == bounds_operation.apply(
                            context.nodes[*input].bounds,
                            SurfaceRect::viewport(context.resources.composition_size),
                        )
                {
                    return Ok(RenderedSurface {
                        rect: input_surface.rect.translate(match bounds_operation {
                            BoundsOperation::Translate(offset) => *offset,
                            _ => unreachable!(),
                        }),
                        ..input_surface
                    });
                }
                let transformed = matches!(
                    bounds_operation,
                    BoundsOperation::Rotate { .. } | BoundsOperation::Perspective { .. }
                );
                let (resources, rect) = self.local_resources(
                    context,
                    node_id,
                    node.bounds,
                    transformed.then_some(input_surface.rect),
                )?;
                if !transformed {
                    self.spatial_composite(
                        encoder,
                        &resources.effect_view_a,
                        rect,
                        resources.size,
                        &input_surface,
                        true,
                    );
                }
                let views = self.render_capability_views(
                    encoder,
                    context,
                    rendered_shared_nodes,
                    capabilities,
                )?;
                let input_views = views.iter().collect::<Vec<_>>();
                let stride = u32::try_from(resources.effect_instance_stride)
                    .map_err(|_| RenderError::backend("effect instance stride exceeds u32"))?;
                let output = if transformed {
                    let pass = passes
                        .first()
                        .ok_or_else(|| RenderError::backend("transform pass is missing"))?;
                    let input = self.effect_input_bind_group(
                        &resources,
                        &input_surface.view,
                        &input_surface.view,
                    );
                    let capability_group =
                        self.capability_bind_group(&input_views, &resources.effect_source_view);
                    let instance_offset = pass.instance.checked_mul(stride).ok_or_else(|| {
                        RenderError::backend("effect instance offset exceeds u32")
                    })?;
                    self.encode_effect_pass(
                        encoder,
                        &resources.effect_view_a,
                        EffectPassBindings {
                            input: &input,
                            capabilities: &capability_group,
                        },
                        &pass.shader,
                        instance_offset,
                    );
                    RenderOutput::EffectA
                } else {
                    self.encode_effect_passes(
                        encoder,
                        &resources,
                        passes,
                        RenderOutput::EffectA,
                        EffectPassContext {
                            stride,
                            capabilities: &input_views,
                        },
                    )?
                };
                let surface = RenderedSurface {
                    view: output.view(&resources)?.clone(),
                    rect,
                    size: resources.size,
                };
                context.local_pool.borrow_mut().push(resources);
                Ok(surface)
            }
            RenderNodeCommandKind::TemporalEffect {
                samples,
                capabilities,
            } => {
                let (resources, rect) =
                    self.local_resources(context, node_id, node.bounds, None)?;
                let held = self.render_capability_views(
                    encoder,
                    context,
                    rendered_shared_nodes,
                    capabilities,
                )?;
                let capability_group = self.capability_bind_group(
                    &held.iter().collect::<Vec<_>>(),
                    &resources.effect_source_view,
                );
                let temporal = resources
                    .temporal
                    .first()
                    .ok_or_else(|| RenderError::backend("temporal render resource is missing"))?;
                self.clear_target(encoder, &temporal.view_a);
                let stride = u32::try_from(resources.effect_instance_stride)
                    .map_err(|_| RenderError::backend("effect instance stride exceeds u32"))?;
                let mut accumulation_is_a = true;
                for (sample, reduce) in samples {
                    let sample =
                        self.render_surface(encoder, context, rendered_shared_nodes, *sample)?;
                    self.spatial_composite(
                        encoder,
                        &resources.effect_view_a,
                        rect,
                        resources.size,
                        &sample,
                        true,
                    );
                    let target_view = if accumulation_is_a {
                        &temporal.view_b
                    } else {
                        &temporal.view_a
                    };
                    let input = &temporal.inputs[usize::from(!accumulation_is_a)];
                    let instance_offset = reduce.instance.checked_mul(stride).ok_or_else(|| {
                        RenderError::backend("temporal instance offset exceeds u32")
                    })?;
                    self.encode_temporal_reduce_pass(
                        encoder,
                        target_view,
                        EffectPassBindings {
                            input,
                            capabilities: &capability_group,
                        },
                        reduce,
                        instance_offset,
                    );
                    accumulation_is_a = !accumulation_is_a;
                }
                let surface = RenderedSurface {
                    view: if accumulation_is_a {
                        temporal.view_a.clone()
                    } else {
                        temporal.view_b.clone()
                    },
                    rect,
                    size: resources.size,
                };
                context.local_pool.borrow_mut().push(resources);
                Ok(surface)
            }
            RenderNodeCommandKind::Composite { children } => {
                let (resources, rect) =
                    self.local_resources(context, node_id, node.bounds, None)?;
                self.clear_target(encoder, &resources.effect_view_a);
                for child in children {
                    match &context.nodes[*child].kind {
                        RenderNodeCommandKind::Source(RenderSourceCommand::Transparent) => {}
                        RenderNodeCommandKind::Source(RenderSourceCommand::Item {
                            shader,
                            instance,
                            capabilities,
                        }) if capabilities.is_empty() => {
                            let inputs =
                                self.capability_bind_group(&[], &resources.effect_source_view);
                            self.encode_item_pass(
                                encoder,
                                &resources.effect_view_a,
                                &resources.bind_group,
                                &inputs,
                                shader,
                                *instance..*instance + 1,
                                wgpu::LoadOp::Load,
                            );
                        }
                        RenderNodeCommandKind::Source(RenderSourceCommand::Texture {
                            index,
                            shader,
                        }) => {
                            let texture = context.textures.get(*index).ok_or_else(|| {
                                RenderError::backend("scene texture resource is missing")
                            })?;
                            self.encode_texture_pass(
                                encoder,
                                &resources.effect_view_a,
                                &texture.binding,
                                shader,
                                wgpu::LoadOp::Load,
                            );
                        }
                        _ => {
                            let surface = self.render_surface(
                                encoder,
                                context,
                                rendered_shared_nodes,
                                *child,
                            )?;
                            self.spatial_composite(
                                encoder,
                                &resources.effect_view_a,
                                rect,
                                resources.size,
                                &surface,
                                false,
                            );
                        }
                    }
                }
                let surface = RenderedSurface {
                    view: resources.effect_view_a.clone(),
                    rect,
                    size: resources.size,
                };
                context.local_pool.borrow_mut().push(resources);
                Ok(surface)
            }
        }
    }

    fn capability_bind_group(
        &self,
        inputs: &[&wgpu::TextureView],
        fallback: &wgpu::TextureView,
    ) -> wgpu::BindGroup {
        let mut entries = (0..capability_input::MAX_INPUTS)
            .map(|index| wgpu::BindGroupEntry {
                binding: index as u32,
                resource: wgpu::BindingResource::TextureView(
                    inputs.get(index).copied().unwrap_or(fallback),
                ),
            })
            .collect::<Vec<_>>();
        entries.push(wgpu::BindGroupEntry {
            binding: capability_input::SAMPLER_BINDING as u32,
            resource: wgpu::BindingResource::Sampler(&self.sampler),
        });
        self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("zerium-capability-inputs"),
            layout: &self.capability_bind_group_layout,
            entries: &entries,
        })
    }

    fn render_capability_views(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        context: &RenderNodeContext<'_>,
        rendered_shared_nodes: &mut [bool],
        capabilities: &[RenderNodeId],
    ) -> Result<Vec<wgpu::TextureView>, RenderError> {
        let mut views = Vec::with_capacity(capabilities.len());
        for capability in capabilities {
            if let RenderNodeCommandKind::Source(RenderSourceCommand::Texture { index, shader }) =
                &context.nodes[*capability].kind
            {
                let texture = context.textures.get(*index).ok_or_else(|| {
                    RenderError::backend("capability texture resource is missing")
                })?;
                self.encode_texture_pass(
                    encoder,
                    &texture.frame_target.view,
                    &texture.binding,
                    shader,
                    wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                );
                views.push(texture.frame_target.view.clone());
            } else {
                views.push(
                    self.render_surface(encoder, context, rendered_shared_nodes, *capability)?
                        .view,
                );
            }
        }
        Ok(views)
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn encode_item_pass(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        target_view: &wgpu::TextureView,
        bind_group: &wgpu::BindGroup,
        capability_group: &wgpu::BindGroup,
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
        pass.set_bind_group(1, capability_group, &[]);
        pass.draw(0..pipeline.vertex_count, instances);
    }

    pub(super) fn encode_texture_pass(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        target_view: &wgpu::TextureView,
        bind_group: &wgpu::BindGroup,
        shader: &ItemShaderId,
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

    fn effect_input_bind_group(
        &self,
        resources: &RenderResources,
        input: &wgpu::TextureView,
        source: &wgpu::TextureView,
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
                    resource: wgpu::BindingResource::TextureView(source),
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

    fn encode_effect_pass(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        target_view: &wgpu::TextureView,
        bindings: EffectPassBindings<'_>,
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
        pass.set_bind_group(0, bindings.input, &[instance_offset]);
        pass.set_bind_group(1, bindings.capabilities, &[]);
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
        bindings: EffectPassBindings<'_>,
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
        pass.set_bind_group(0, bindings.input, &[instance_offset]);
        pass.set_bind_group(1, bindings.capabilities, &[]);
        pass.draw(0..pipeline.vertex_count, 0..1);
    }

    pub(super) fn encode_compute_pass(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        resources: &RenderResources,
        input: &wgpu::BindGroup,
        capability_group: &wgpu::BindGroup,
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
        pass.set_bind_group(1, capability_group, &[]);
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
        mut output: RenderOutput,
        context: EffectPassContext<'_>,
    ) -> Result<RenderOutput, RenderError> {
        let capability_group =
            self.capability_bind_group(context.capabilities, &resources.effect_source_view);
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
                .checked_mul(context.stride)
                .ok_or_else(|| RenderError::backend("effect instance offset exceeds u32"))?;
            let target = output.next_effect_target();
            let target_view = target.view(resources)?;
            let effect_input = match output {
                RenderOutput::EffectA => &resources.effect_input_a,
                RenderOutput::EffectB => &resources.effect_input_b,
            };
            match effect_pass.kind {
                EffectPassCommandKind::Render => self.encode_effect_pass(
                    encoder,
                    target_view,
                    EffectPassBindings {
                        input: effect_input,
                        capabilities: &capability_group,
                    },
                    &effect_pass.shader,
                    instance_offset,
                ),
                EffectPassCommandKind::Compute { .. } => {
                    let compute_input = match output {
                        RenderOutput::EffectA => &resources.compute_inputs[0],
                        RenderOutput::EffectB => &resources.compute_inputs[1],
                    };
                    self.encode_compute_pass(
                        encoder,
                        resources,
                        compute_input,
                        &capability_group,
                        effect_pass,
                    )?
                }
            }
            output = target;
        }
        Ok(output)
    }

    fn encode_scene(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        target_view: &wgpu::TextureView,
        resources_by_scale: &HashMap<u32, RenderResources>,
        textures: &[TextureResource],
        scene: &EncodedScene,
        available_resources: Vec<RenderResources>,
    ) -> Result<SceneEncoding, RenderError> {
        let local_pool = RefCell::new(Vec::new());
        let available_local_resources = RefCell::new(available_resources);
        let decode = |value: f64| {
            if value <= 0.04045 {
                value / 12.92
            } else {
                ((value + 0.055) / 1.055).powf(2.4)
            }
        };
        let background = wgpu::Color {
            r: decode(scene.background[0]),
            g: decode(scene.background[1]),
            b: decode(scene.background[2]),
            a: scene.background[3],
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
                    let capability_group =
                        self.capability_bind_group(&[], &base_resources.effect_source_view);
                    self.encode_item_pass(
                        encoder,
                        target_view,
                        &base_resources.bind_group,
                        &capability_group,
                        &batch.shader,
                        batch.instances.clone(),
                        wgpu::LoadOp::Load,
                    );
                }
                RenderCommand::Texture { index, shader } => {
                    let texture = textures.get(*index).ok_or_else(|| {
                        RenderError::backend("video render command references a missing frame")
                    })?;
                    let bind_group = &texture.binding;
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
                    let context = RenderNodeContext {
                        resources,
                        encoded: scene,
                        viewport: resources.output_size,
                        render_scale: *render_scale,
                        local_pool: &local_pool,
                        available_local_resources: &available_local_resources,
                        textures,
                        nodes: &scene.nodes,
                        shared_node_slots: &scene.shared_node_slots,
                    };
                    let surface =
                        self.render_surface(encoder, &context, rendered_shared_nodes, *node)?;
                    self.spatial_composite(
                        encoder,
                        target_view,
                        SurfaceRect::viewport(resources.composition_size),
                        resources.output_size,
                        &surface,
                        false,
                    );
                }
            }
        }
        Ok(SceneEncoding {
            cache_updates: rendered_by_scale
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
                .collect(),
            local_resources: local_pool.into_inner(),
            unused_resources: available_local_resources.into_inner(),
        })
    }

    pub(crate) fn render_to_view(
        &self,
        scene: &RenderScene,
        target_view: &wgpu::TextureView,
    ) -> Result<wgpu::SubmissionIndex, RenderError> {
        self.validate_scene(scene)?;
        let encoded = encode_items(scene)?;
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
                            frame_output: true,
                            item_count: encoded.items.len(),
                            property_size: encoded.properties.len(),
                            effect_pass_count: 0,
                            effect_property_size: 0,
                            temporal_depth: 0,
                            shared_node_count,
                        },
                    )?,
                );
            }
        }

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
        let available_local_resources = {
            let mut cached = self
                .local_resources
                .lock()
                .map_err(|_| RenderError::backend("local render resource lock poisoned"))?;
            std::mem::take(&mut *cached)
        };
        let scene_encoding = self.encode_scene(
            &mut encoder,
            &scene_view,
            &resources,
            &texture_resources,
            &encoded,
            available_local_resources,
        )?;
        let base_resources = resources
            .get(&1)
            .expect("scale-one render resources were created");
        self.encode_output_pass(&mut encoder, target_view, &base_resources.output_input);
        let submission = self.queue.submit([encoder.finish()]);
        for (scale, slot, key) in scene_encoding.cache_updates {
            if let Some(resources) = resources.get_mut(&scale) {
                resources.cached_node_keys[slot] = Some(key);
            }
        }
        {
            let mut cached = self
                .local_resources
                .lock()
                .map_err(|_| RenderError::backend("local render resource lock poisoned"))?;
            let mut retained_bytes = 0_usize;
            for resource in scene_encoding
                .local_resources
                .into_iter()
                .chain(scene_encoding.unused_resources)
            {
                let surfaces = 4_usize.saturating_add(resource.temporal.len().saturating_mul(2));
                let bytes = (resource.size.width as usize)
                    .saturating_mul(resource.size.height as usize)
                    .saturating_mul(8)
                    .saturating_mul(surfaces);
                if retained_bytes.saturating_add(bytes) <= LOCAL_RESOURCE_CACHE_BUDGET {
                    retained_bytes += bytes;
                    cached.push(resource);
                }
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
