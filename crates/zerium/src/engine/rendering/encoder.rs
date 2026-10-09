use super::FrameRenderer;
use super::encoded_scene::{
    EffectPassCommand, EffectPassCommandKind, EncodedScene, GpuComposite, GpuEffect, RenderCommand,
    RenderNodeCommandKind, RenderNodeId, RenderNodeKey, RenderSourceCommand, TemporalReduceCommand,
    encode_scene,
};
use super::resources::{CachedNode, FrameResources, NodeResources, RenderTarget, TextureResource};
use super::scene::{RenderError, RenderScene, RenderSize, RenderView};
use super::surface::SurfaceRect;
use std::collections::{HashMap, HashSet};
use std::num::NonZeroU64;
use std::ops::Range;
use std::sync::Arc;
use zerium_core::plugin::{ComputeDispatchDimension, EffectInputSpace};
use zerium_core::timeline::BlendMode;
use zerium_shader::{EffectShaderId, ItemShaderId, capability_input};

#[derive(Clone)]
struct RenderedSurface {
    view: wgpu::TextureView,
    rect: SurfaceRect,
    size: RenderSize,
}

struct FrameEncoding {
    cache_updates: Vec<(u32, usize, Arc<RenderNodeKey>)>,
    // Keep transient surfaces alive until their GPU commands are submitted.
    local_resources: Vec<NodeResources>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RenderOutput {
    EffectA,
    EffectB,
}

impl RenderOutput {
    fn view(self, resources: &NodeResources) -> &wgpu::TextureView {
        match self {
            Self::EffectA => &resources.targets[0].view,
            Self::EffectB => &resources.targets[1].view,
        }
    }

    fn texture(self, resources: &NodeResources) -> &wgpu::Texture {
        match self {
            Self::EffectA => &resources.targets[0].texture,
            Self::EffectB => &resources.targets[1].texture,
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

struct RenderNodeContext<'a> {
    frame: &'a FrameResources,
    cached_nodes: &'a [CachedNode],
    encoded: &'a EncodedScene,
    render_scale: u32,
    local_pool: &'a mut Vec<NodeResources>,
    available_local_resources: &'a mut Vec<NodeResources>,
    rendered_shared_nodes: &'a mut [bool],
    textures: &'a [TextureResource],
}

struct PassBindings<'a> {
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
        context: &mut RenderNodeContext<'_>,
        node_id: RenderNodeId,
        bounds: SurfaceRect,
        input_bounds: Option<SurfaceRect>,
    ) -> Result<(NodeResources, SurfaceRect), RenderError> {
        let composition = context.frame.composition_size;
        let (rect, size) = bounds
            .pixel_aligned(context.frame.size, composition, context.render_scale)
            .ok_or_else(|| {
                RenderError::resource_limit("render surface bounds cannot be rasterized")
            })?;
        let limit = self.shared.device.limits().max_texture_dimension_2d;
        if size.width > limit || size.height > limit {
            return Err(RenderError::resource_limit(format!(
                "render surface {}x{} exceeds GPU limit {limit}",
                size.width, size.height
            )));
        }
        let encoded = context.encoded;
        let needs_temporal = matches!(
            encoded.nodes[node_id].kind,
            RenderNodeCommandKind::TemporalEffect { .. }
        );
        let reuse = context
            .available_local_resources
            .iter()
            .position(|resource| resource.satisfies(size, composition, encoded, needs_temporal));
        let resources = if let Some(index) = reuse {
            context.available_local_resources.swap_remove(index)
        } else {
            self.create_node_resources(size, composition, encoded, needs_temporal)?
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
            self.shared.queue.write_buffer(
                &resources.items.item_buffer,
                0,
                bytemuck::cast_slice(&items),
            );
        }
        if !encoded.properties.is_empty() {
            self.shared.queue.write_buffer(
                &resources.items.property_buffer,
                0,
                &encoded.properties,
            );
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
            self.shared.queue.write_buffer(
                &resources.effect_instance_buffer,
                0,
                &Self::effect_instance_data(&effects, stride),
            );
            let compute_stride = usize::try_from(resources.compute_info_stride)
                .map_err(|_| RenderError::backend("compute info stride exceeds usize"))?;
            self.shared.queue.write_buffer(
                &resources.compute_info_buffer,
                0,
                &Self::compute_instance_data(&effects, compute_stride, size, composition),
            );
            if !encoded.effect_properties.is_empty() {
                self.shared.queue.write_buffer(
                    &resources.effect_property_buffer,
                    0,
                    &encoded.effect_properties,
                );
            }
        }
        Ok((resources, rect))
    }

    fn clear_target(&self, encoder: &mut wgpu::CommandEncoder, target: &wgpu::TextureView) {
        render_pass(
            encoder,
            target,
            wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
            "zerium-surface-clear",
        );
    }

    fn spatial_composite(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        target_rect: SurfaceRect,
        target_size: RenderSize,
        source: &RenderedSurface,
        view: RenderView,
    ) {
        let info = GpuComposite::new(
            source.size,
            source.rect,
            target_size,
            target_rect,
            view,
            BlendMode::Normal,
        );
        self.encode_composite_pass(encoder, target, &source.view, info, None);
    }

    fn composite_layer(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        target: &RenderTarget,
        backdrop: Option<&RenderTarget>,
        source: &RenderedSurface,
        info: GpuComposite,
    ) {
        let backdrop = (info.blend_mode[0] != BlendMode::Normal as u32).then(|| {
            let backdrop = backdrop.expect("non-normal layers have a backdrop texture");
            encoder.copy_texture_to_texture(
                target.texture.as_image_copy(),
                backdrop.texture.as_image_copy(),
                wgpu::Extent3d {
                    width: info.output_size[0],
                    height: info.output_size[1],
                    depth_or_array_layers: 1,
                },
            );
            &backdrop.view
        });
        self.encode_composite_pass(encoder, &target.view, &source.view, info, backdrop);
    }

    fn render_surface(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        context: &mut RenderNodeContext<'_>,
        node_id: RenderNodeId,
    ) -> Result<RenderedSurface, RenderError> {
        if let Some(slot) = context.encoded.shared_node_slots[node_id] {
            let cached = context
                .cached_nodes
                .get(slot)
                .ok_or_else(|| RenderError::backend("shared render node resource is missing"))?;
            let rect = SurfaceRect::viewport(context.frame.composition_size);
            if !context.rendered_shared_nodes[slot] {
                let surface = self.render_surface_uncached(encoder, context, node_id)?;
                self.clear_target(encoder, &cached.target.view);
                self.spatial_composite(
                    encoder,
                    &cached.target.view,
                    rect,
                    context
                        .frame
                        .size
                        .checked_scale(context.render_scale)
                        .expect("render scale was validated"),
                    &surface,
                    RenderView::default(),
                );
                context.rendered_shared_nodes[slot] = true;
            }
            return Ok(RenderedSurface {
                view: cached.target.view.clone(),
                rect,
                size: context
                    .frame
                    .size
                    .checked_scale(context.render_scale)
                    .expect("render scale was validated"),
            });
        }
        self.render_surface_uncached(encoder, context, node_id)
    }

    fn render_surface_uncached(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        context: &mut RenderNodeContext<'_>,
        node_id: RenderNodeId,
    ) -> Result<RenderedSurface, RenderError> {
        let encoded = context.encoded;
        let node = &encoded.nodes[node_id];
        match &node.kind {
            RenderNodeCommandKind::Source(source) => {
                let (resources, rect) =
                    self.local_resources(context, node_id, node.bounds, None)?;
                match source {
                    RenderSourceCommand::Transparent => {
                        self.clear_target(encoder, &resources.targets[0].view)
                    }
                    RenderSourceCommand::Item {
                        shader,
                        instance,
                        capabilities,
                    } => {
                        let views = self.render_capability_views(encoder, context, capabilities)?;
                        let inputs = self.capability_bind_group(
                            &views.iter().collect::<Vec<_>>(),
                            &resources.source.view,
                        );
                        self.encode_item_pass(
                            encoder,
                            &resources.targets[0].view,
                            PassBindings {
                                input: &resources.items.bind_group,
                                capabilities: &inputs,
                            },
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
                            &resources.targets[0].view,
                            &texture.binding,
                            shader,
                            wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        );
                    }
                }
                let surface = RenderedSurface {
                    view: resources.targets[0].view.clone(),
                    rect,
                    size: resources.size,
                };
                context.local_pool.push(resources);
                Ok(surface)
            }
            RenderNodeCommandKind::Effect {
                input,
                capabilities,
                input_space,
                passes,
            } => {
                let input_surface = self.render_surface(encoder, context, *input)?;
                let transformed = *input_space == EffectInputSpace::Source;
                let (resources, rect) = self.local_resources(
                    context,
                    node_id,
                    node.bounds,
                    transformed.then_some(input_surface.rect),
                )?;
                if !transformed {
                    self.clear_target(encoder, &resources.targets[0].view);
                    self.spatial_composite(
                        encoder,
                        &resources.targets[0].view,
                        rect,
                        resources.size,
                        &input_surface,
                        RenderView::default(),
                    );
                }
                let views = self.render_capability_views(encoder, context, capabilities)?;
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
                        self.capability_bind_group(&input_views, &resources.source.view);
                    let instance_offset = pass.instance.checked_mul(stride).ok_or_else(|| {
                        RenderError::backend("effect instance offset exceeds u32")
                    })?;
                    self.encode_effect_pass(
                        encoder,
                        &resources.targets[0].view,
                        PassBindings {
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
                    view: output.view(&resources).clone(),
                    rect,
                    size: resources.size,
                };
                context.local_pool.push(resources);
                Ok(surface)
            }
            RenderNodeCommandKind::TemporalEffect {
                samples,
                capabilities,
            } => {
                let (resources, rect) =
                    self.local_resources(context, node_id, node.bounds, None)?;
                let held = self.render_capability_views(encoder, context, capabilities)?;
                let capability_group = self.capability_bind_group(
                    &held.iter().collect::<Vec<_>>(),
                    &resources.source.view,
                );
                let temporal = resources
                    .temporal
                    .as_ref()
                    .ok_or_else(|| RenderError::backend("temporal render resource is missing"))?;
                self.clear_target(encoder, &temporal.targets[0].view);
                let stride = u32::try_from(resources.effect_instance_stride)
                    .map_err(|_| RenderError::backend("effect instance stride exceeds u32"))?;
                let mut accumulation = 0;
                for (sample, reduce) in samples {
                    let sample = self.render_surface(encoder, context, *sample)?;
                    self.clear_target(encoder, &resources.targets[0].view);
                    self.spatial_composite(
                        encoder,
                        &resources.targets[0].view,
                        rect,
                        resources.size,
                        &sample,
                        RenderView::default(),
                    );
                    let target_view = &temporal.targets[1 - accumulation].view;
                    let input = &temporal.inputs[accumulation];
                    let instance_offset = reduce.instance.checked_mul(stride).ok_or_else(|| {
                        RenderError::backend("temporal instance offset exceeds u32")
                    })?;
                    self.encode_temporal_reduce_pass(
                        encoder,
                        target_view,
                        PassBindings {
                            input,
                            capabilities: &capability_group,
                        },
                        reduce,
                        instance_offset,
                    );
                    accumulation = 1 - accumulation;
                }
                let surface = RenderedSurface {
                    view: temporal.targets[accumulation].view.clone(),
                    rect,
                    size: resources.size,
                };
                context.local_pool.push(resources);
                Ok(surface)
            }
            RenderNodeCommandKind::Composite { children, view } => {
                let (resources, rect) =
                    self.local_resources(context, node_id, node.bounds, None)?;
                self.clear_target(encoder, &resources.targets[0].view);
                for child in children {
                    match &context.encoded.nodes[child.node].kind {
                        RenderNodeCommandKind::Source(RenderSourceCommand::Transparent) => {}
                        RenderNodeCommandKind::Source(RenderSourceCommand::Item {
                            shader,
                            instance,
                            capabilities,
                        }) if capabilities.is_empty()
                            && *view == RenderView::default()
                            && child.blend_mode == BlendMode::Normal =>
                        {
                            let inputs = self.capability_bind_group(&[], &resources.source.view);
                            self.encode_item_pass(
                                encoder,
                                &resources.targets[0].view,
                                PassBindings {
                                    input: &resources.items.bind_group,
                                    capabilities: &inputs,
                                },
                                shader,
                                *instance..*instance + 1,
                                wgpu::LoadOp::Load,
                            );
                        }
                        _ => {
                            let surface = self.render_surface(encoder, context, child.node)?;
                            self.composite_layer(
                                encoder,
                                &resources.targets[0],
                                Some(&resources.targets[1]),
                                &surface,
                                GpuComposite::new(
                                    surface.size,
                                    surface.rect,
                                    resources.size,
                                    rect,
                                    *view,
                                    child.blend_mode,
                                ),
                            );
                        }
                    }
                }
                let surface = RenderedSurface {
                    view: resources.targets[0].view.clone(),
                    rect,
                    size: resources.size,
                };
                context.local_pool.push(resources);
                Ok(surface)
            }
        }
    }

    pub(super) fn capability_bind_group(
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
            resource: wgpu::BindingResource::Sampler(&self.shared.sampler),
        });
        self.shared
            .device
            .create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("zerium-capability-inputs"),
                layout: &self.shared.capability_bind_group_layout,
                entries: &entries,
            })
    }

    fn render_capability_views(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        context: &mut RenderNodeContext<'_>,
        capabilities: &[RenderNodeId],
    ) -> Result<Vec<wgpu::TextureView>, RenderError> {
        let mut views = Vec::with_capacity(capabilities.len());
        for capability in capabilities {
            if let RenderNodeCommandKind::Source(RenderSourceCommand::Texture { index, shader }) =
                &context.encoded.nodes[*capability].kind
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
                views.push(self.render_surface(encoder, context, *capability)?.view);
            }
        }
        Ok(views)
    }

    fn encode_item_pass(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        target_view: &wgpu::TextureView,
        bindings: PassBindings<'_>,
        shader: &ItemShaderId,
        instances: Range<u32>,
        load: wgpu::LoadOp<wgpu::Color>,
    ) {
        let mut pass = render_pass(encoder, target_view, load, "zerium-item-pass");
        let pipeline = self
            .shared
            .pipelines
            .get(shader)
            .expect("scene shaders were validated before encoding");
        pass.set_pipeline(&pipeline.pipeline);
        pass.set_bind_group(0, bindings.input, &[]);
        pass.set_bind_group(1, bindings.capabilities, &[]);
        pass.draw(0..pipeline.vertex_count, instances);
    }

    fn encode_texture_pass(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        target_view: &wgpu::TextureView,
        bind_group: &wgpu::BindGroup,
        shader: &ItemShaderId,
        load: wgpu::LoadOp<wgpu::Color>,
    ) {
        let mut pass = render_pass(encoder, target_view, load, "zerium-video-pass");
        let pipeline = self
            .shared
            .texture_pipelines
            .get(shader)
            .expect("scene texture item shaders were validated before encoding");
        pass.set_pipeline(&pipeline.pipeline);
        pass.set_bind_group(0, bind_group, &[]);
        pass.draw(0..pipeline.vertex_count, 0..1);
    }

    fn effect_input_bind_group(
        &self,
        resources: &NodeResources,
        input: &wgpu::TextureView,
        source: &wgpu::TextureView,
    ) -> wgpu::BindGroup {
        self.shared
            .device
            .create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("zerium-dynamic-effect-input"),
                layout: &self.shared.effect_bind_group_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(input),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(&self.shared.sampler),
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

    pub(super) fn composite_input_bind_group(
        &self,
        input: &wgpu::TextureView,
        info: &wgpu::Buffer,
    ) -> wgpu::BindGroup {
        self.shared
            .device
            .create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("zerium-dynamic-composite-input"),
                layout: &self.shared.composite_bind_group_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(input),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(&self.shared.sampler),
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
        bindings: PassBindings<'_>,
        shader: &EffectShaderId,
        instance_offset: u32,
    ) {
        let mut pass = render_pass(
            encoder,
            target_view,
            wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
            "zerium-effect-pass",
        );
        let pipeline = self
            .shared
            .effect_pipelines
            .get(shader)
            .expect("scene effect shaders were validated before encoding");
        pass.set_pipeline(&pipeline.pipeline);
        pass.set_bind_group(0, bindings.input, &[instance_offset]);
        pass.set_bind_group(1, bindings.capabilities, &[]);
        pass.draw(0..pipeline.vertex_count, 0..1);
    }

    fn encode_composite_pass(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        target_view: &wgpu::TextureView,
        source: &wgpu::TextureView,
        info: GpuComposite,
        backdrop: Option<&wgpu::TextureView>,
    ) {
        let buffer = self.shared.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("zerium-spatial-composite-info"),
            size: size_of::<GpuComposite>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        self.shared
            .queue
            .write_buffer(&buffer, 0, bytemuck::bytes_of(&info));
        let input = self.composite_input_bind_group(source, &buffer);
        let backdrop = backdrop.map(|view| {
            self.shared
                .device
                .create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("zerium-blend-backdrop"),
                    layout: &self.shared.backdrop_bind_group_layout,
                    entries: &[wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(view),
                    }],
                })
        });
        let mut pass = render_pass(
            encoder,
            target_view,
            wgpu::LoadOp::Load,
            "zerium-effect-composite-pass",
        );
        if let Some(backdrop) = &backdrop {
            pass.set_pipeline(&self.shared.blend_pipeline);
            pass.set_bind_group(1, backdrop, &[]);
        } else {
            pass.set_pipeline(&self.shared.composite_pipeline);
        }
        pass.set_bind_group(0, &input, &[]);
        pass.draw(0..3, 0..1);
    }

    fn encode_output_pass(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        target_view: &wgpu::TextureView,
        input: &wgpu::BindGroup,
    ) {
        let mut pass = render_pass(
            encoder,
            target_view,
            wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
            "zerium-output-transform-pass",
        );
        pass.set_pipeline(&self.shared.output_pipeline);
        pass.set_bind_group(0, input, &[]);
        pass.draw(0..3, 0..1);
    }

    fn encode_temporal_reduce_pass(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        target_view: &wgpu::TextureView,
        bindings: PassBindings<'_>,
        command: &TemporalReduceCommand,
        instance_offset: u32,
    ) {
        let mut pass = render_pass(
            encoder,
            target_view,
            wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
            "zerium-temporal-reduce-pass",
        );
        let pipeline = self
            .shared
            .temporal_pipelines
            .get(&command.reducer)
            .expect("temporal shaders were validated before encoding");
        pass.set_pipeline(&pipeline.pipeline);
        pass.set_bind_group(0, bindings.input, &[instance_offset]);
        pass.set_bind_group(1, bindings.capabilities, &[]);
        pass.draw(0..pipeline.vertex_count, 0..1);
    }

    fn encode_compute_pass(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        resources: &NodeResources,
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
            .shared
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
        resources: &NodeResources,
        passes: &[EffectPassCommand],
        mut output: RenderOutput,
        context: EffectPassContext<'_>,
    ) -> Result<RenderOutput, RenderError> {
        let capability_group =
            self.capability_bind_group(context.capabilities, &resources.source.view);
        for effect_pass in passes {
            if effect_pass.captures_source {
                encoder.copy_texture_to_texture(
                    wgpu::TexelCopyTextureInfo {
                        texture: output.texture(resources),
                        mip_level: 0,
                        origin: wgpu::Origin3d::ZERO,
                        aspect: wgpu::TextureAspect::All,
                    },
                    wgpu::TexelCopyTextureInfo {
                        texture: &resources.source.texture,
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
            let target_view = target.view(resources);
            let effect_input = match output {
                RenderOutput::EffectA => &resources.effect_inputs[0],
                RenderOutput::EffectB => &resources.effect_inputs[1],
            };
            match effect_pass.kind {
                EffectPassCommandKind::Render => self.encode_effect_pass(
                    encoder,
                    target_view,
                    PassBindings {
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

    fn encode_frame(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        resources: &FrameResources,
        textures: &[TextureResource],
        scene: &EncodedScene,
        available_resources: Vec<NodeResources>,
    ) -> Result<FrameEncoding, RenderError> {
        let mut local_pool = Vec::new();
        let mut available_local_resources = available_resources;
        let target_view = &resources.scene.view;
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
        render_pass(
            encoder,
            target_view,
            wgpu::LoadOp::Clear(background),
            "zerium-frame-clear-pass",
        );

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
                    self.encode_item_pass(
                        encoder,
                        target_view,
                        PassBindings {
                            input: &resources.items.bind_group,
                            capabilities: &resources.empty_capabilities,
                        },
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
                RenderCommand::Surface {
                    layer,
                    render_scale,
                } => {
                    let cached_nodes = &resources.node_caches[render_scale];
                    let rendered_shared_nodes =
                        rendered_by_scale.entry(*render_scale).or_insert_with(|| {
                            cached_nodes
                                .iter()
                                .zip(&keys_by_slot)
                                .map(|(cached, expected)| {
                                    &cached.key == expected && expected.is_some()
                                })
                                .collect()
                        });
                    let mut context = RenderNodeContext {
                        frame: resources,
                        cached_nodes,
                        encoded: scene,
                        render_scale: *render_scale,
                        local_pool: &mut local_pool,
                        available_local_resources: &mut available_local_resources,
                        rendered_shared_nodes,
                        textures,
                    };
                    let surface = self.render_surface(encoder, &mut context, layer.node)?;
                    self.composite_layer(
                        encoder,
                        &resources.scene,
                        resources.backdrop.as_ref(),
                        &surface,
                        GpuComposite::new(
                            surface.size,
                            surface.rect,
                            resources.size,
                            SurfaceRect::viewport(resources.composition_size),
                            RenderView::default(),
                            layer.blend_mode,
                        ),
                    );
                }
            }
        }
        Ok(FrameEncoding {
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
            local_resources: local_pool
                .into_iter()
                .chain(available_local_resources)
                .collect(),
        })
    }

    pub(crate) fn render_to_view(
        &self,
        scene: &RenderScene,
        target_view: &wgpu::TextureView,
    ) -> Result<wgpu::SubmissionIndex, RenderError> {
        self.validate_scene(scene)?;
        let encoded = encode_scene(scene)?;
        let shared_node_count = encoded.shared_node_slots.iter().flatten().count();
        let texture_resources = self.create_texture_resources(&encoded.textures)?;
        let mut frame_resources = self
            .frame_resources
            .lock()
            .map_err(|_| RenderError::backend("render resource lock poisoned"))?;
        if frame_resources
            .as_ref()
            .is_none_or(|frame| !frame.satisfies(scene, &encoded))
        {
            *frame_resources = Some(self.create_frame_resources(scene, &encoded)?);
        }
        let frame = frame_resources
            .as_mut()
            .expect("frame resources were created");
        if frame.backdrop.is_none() && encoded.commands.iter().any(|command| {
            matches!(command, RenderCommand::Surface { layer, .. } if layer.blend_mode != BlendMode::Normal)
        }) {
            frame.backdrop = Some(RenderTarget::new(
                &self.shared.device,
                scene.size,
                wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                "zerium-frame-backdrop",
            ));
        }
        let required_scales = encoded
            .commands
            .iter()
            .filter_map(|command| match command {
                RenderCommand::Surface { render_scale, .. } => Some(*render_scale),
                _ => None,
            })
            .collect::<HashSet<_>>();
        frame
            .node_caches
            .retain(|scale, _| required_scales.contains(scale));
        for scale in required_scales {
            let size = scene
                .size
                .checked_scale(scale)
                .ok_or_else(|| RenderError::resource_limit("render scale overflows"))?;
            let cached = frame.node_caches.entry(scale).or_default();
            if cached.len() < shared_node_count {
                *cached = (0..shared_node_count)
                    .map(|_| CachedNode {
                        target: RenderTarget::new(
                            &self.shared.device,
                            size,
                            wgpu::TextureUsages::TEXTURE_BINDING
                                | wgpu::TextureUsages::RENDER_ATTACHMENT,
                            "zerium-shared-render-node",
                        ),
                        key: None,
                    })
                    .collect();
            }
        }
        if !encoded.items.is_empty() {
            self.shared.queue.write_buffer(
                &frame.items.item_buffer,
                0,
                bytemuck::cast_slice(&encoded.items),
            );
        }
        if !encoded.properties.is_empty() {
            self.shared
                .queue
                .write_buffer(&frame.items.property_buffer, 0, &encoded.properties);
        }

        let mut encoder =
            self.shared
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("zerium-preview-frame-encoder"),
                });
        let available_local_resources = {
            let mut cached = self
                .local_resources
                .lock()
                .map_err(|_| RenderError::backend("local render resource lock poisoned"))?;
            std::mem::take(&mut *cached)
        };
        let scene_encoding = self.encode_frame(
            &mut encoder,
            frame,
            &texture_resources,
            &encoded,
            available_local_resources,
        )?;
        self.encode_output_pass(&mut encoder, target_view, &frame.output_input);
        let submission = self.shared.queue.submit([encoder.finish()]);
        for (scale, slot, key) in scene_encoding.cache_updates {
            frame
                .node_caches
                .get_mut(&scale)
                .expect("render cache scale exists")[slot]
                .key = Some(key);
        }
        self.recycle_local_resources(scene_encoding.local_resources)?;
        self.recycle_texture_resources(texture_resources)?;
        Ok(submission)
    }
}

pub(super) fn render_pass<'a>(
    encoder: &'a mut wgpu::CommandEncoder,
    target: &wgpu::TextureView,
    load: wgpu::LoadOp<wgpu::Color>,
    label: &str,
) -> wgpu::RenderPass<'a> {
    encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some(label),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view: target,
            depth_slice: None,
            resolve_target: None,
            ops: wgpu::Operations {
                load,
                store: wgpu::StoreOp::Store,
            },
        })],
        ..Default::default()
    })
}
