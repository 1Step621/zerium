use super::RenderSession;
use super::bindings::{GpuComposite, GpuImage, GpuPass, GpuSurface};
use super::passes::{PassBindings, render_pass};
use super::plan::{
    ImageInput, PassKind, RenderCommand, RenderNodeCommandKind, RenderNodeId, RenderPlan,
    RenderSourceCommand, plan_scene,
};
use super::resources::{BackdropPool, FrameResources, NodeResources, RenderTarget};
use super::scene::{RasterGrid, SurfaceRect};
use super::scene::{RenderError, RenderScene, RenderSize, RenderView};
use super::surfaces::{
    FrameSurfaces, RenderRequest, RenderedSurface, SurfaceOutput, SurfaceRaster,
};
use super::textures::FrameImage;
use zerium_core::timeline::BlendMode;
use zerium_shader::{IMAGE_SAMPLER_BINDING, ImageBinding};

struct FrameEncoding {
    surfaces: FrameSurfaces,
    backdrops: Vec<RenderTarget>,
    unused_resources: Vec<NodeResources>,
}

struct RenderNodeContext<'a> {
    frame: &'a FrameResources,
    plan: &'a RenderPlan,
    available_resources: &'a mut Vec<NodeResources>,
    surfaces: &'a mut FrameSurfaces,
    backdrops: &'a mut BackdropPool,
    frame_images: &'a mut [FrameImage],
}

impl RenderSession {
    fn local_resources(
        &self,
        context: &mut RenderNodeContext<'_>,
        node_id: RenderNodeId,
        raster: SurfaceRaster,
    ) -> NodeResources {
        let (rect, size) = (raster.rect(), raster.size());
        let plan = context.plan;
        let uniform_size = match &plan.nodes[node_id].kind {
            RenderNodeCommandKind::Source(RenderSourceCommand::Item { .. }) => {
                size_of::<GpuSurface>() as u64
            }
            RenderNodeCommandKind::Pass { .. } => size_of::<GpuPass>() as u64,
            _ => 0,
        };
        let reuse = context
            .available_resources
            .iter()
            .position(|resource| resource.satisfies(size, uniform_size));
        let resources = if let Some(index) = reuse {
            context.available_resources.swap_remove(index)
        } else {
            self.create_node_resources(size, uniform_size)
        };
        let surface_size = [
            (rect.max[0] - rect.min[0]) as f32,
            (rect.max[1] - rect.min[1]) as f32,
        ];
        let surface_min = [rect.min[0] as f32, rect.min[1] as f32];
        if let RenderNodeCommandKind::Source(RenderSourceCommand::Item { .. }) =
            &plan.nodes[node_id].kind
        {
            self.shared.gpu.queue.write_buffer(
                resources
                    .uniform
                    .as_ref()
                    .expect("item nodes have a uniform"),
                0,
                bytemuck::bytes_of(&GpuSurface {
                    output_size: [size.width as f32, size.height as f32],
                    composition_size: [
                        context.frame.composition_size.width as f32,
                        context.frame.composition_size.height as f32,
                    ],
                    surface_min,
                    surface_size,
                }),
            );
        }

        if let RenderNodeCommandKind::Pass { pass, .. } = &plan.nodes[node_id].kind {
            let mut uniform = plan.passes[pass.instance as usize];
            uniform.surface_min = surface_min;
            uniform.surface_size = surface_size;
            uniform.output_size = [size.width, size.height];
            uniform.bounds_min = plan.nodes[node_id].bounds.min.map(|value| value as f32);
            uniform.bounds_size = [
                (plan.nodes[node_id].bounds.max[0] - plan.nodes[node_id].bounds.min[0]) as f32,
                (plan.nodes[node_id].bounds.max[1] - plan.nodes[node_id].bounds.min[1]) as f32,
            ];
            self.shared.gpu.queue.write_buffer(
                resources
                    .uniform
                    .as_ref()
                    .expect("pass nodes have a uniform"),
                0,
                bytemuck::bytes_of(&uniform),
            );
        }

        resources
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
        backdrops: &mut BackdropPool,
        source: &RenderedSurface,
        info: GpuComposite,
    ) {
        let backdrop = (info.blend_mode[0] != BlendMode::Normal as u32).then(|| {
            let backdrop = backdrops.get(
                &self.shared.gpu.device,
                RenderSize {
                    width: info.output_size[0],
                    height: info.output_size[1],
                },
            );
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
        request: RenderRequest,
    ) -> Result<RenderedSurface, RenderError> {
        let node = &context.plan.nodes[node_id];
        let Some(output) = SurfaceOutput::resolve(
            node,
            request,
            self.shared.gpu.device.limits().max_texture_dimension_2d,
        )?
        else {
            return Ok(RenderedSurface {
                bounds: context.plan.nodes[node_id].bounds,
                view: context.frame.transparent.view.clone(),
                rect: SurfaceRect {
                    min: [0.; 2],
                    max: [1.; 2],
                },
                size: RenderSize {
                    width: 1,
                    height: 1,
                },
            });
        };

        let key = output.key(context.plan.node_keys[node_id]);
        if let Some(surface) = context.surfaces.get(key) {
            return Ok(surface);
        }
        let (surface, resources) =
            self.render_surface_uncached(encoder, context, node_id, output)?;
        context.surfaces.insert(key, surface.clone(), resources);
        Ok(surface)
    }

    fn render_native_frame(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        context: &mut RenderNodeContext<'_>,
        index: usize,
        bounds: SurfaceRect,
    ) -> Result<RenderedSurface, RenderError> {
        let texture = context
            .frame_images
            .get_mut(index)
            .ok_or_else(|| RenderError::backend("image frame is missing"))?;
        let frame = &texture.uploaded.frame;
        let size = RenderSize {
            width: frame.width,
            height: frame.height,
        };
        let target = texture.target.get_or_insert_with(|| {
            RenderTarget::new(
                &self.shared.gpu.device,
                size,
                wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
                "zerium-image-frame",
            )
        });
        self.encode_frame_pass(
            encoder,
            &target.view,
            &texture.binding,
            wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
        );
        Ok(RenderedSurface {
            bounds,
            rect: bounds,
            size,
            view: target.view.clone(),
        })
    }

    fn render_surface_uncached(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        context: &mut RenderNodeContext<'_>,
        node_id: RenderNodeId,
        output: SurfaceOutput,
    ) -> Result<(RenderedSurface, Option<NodeResources>), RenderError> {
        let node = &context.plan.nodes[node_id];
        let raster = match output {
            SurfaceOutput::NativeFrame { index } => {
                return Ok((
                    self.render_native_frame(encoder, context, index, node.bounds)?,
                    None,
                ));
            }
            SurfaceOutput::Raster(raster) => raster,
        };
        let (rect, grid) = (raster.rect(), raster.grid());
        let resources = self.local_resources(context, node_id, raster);

        match &node.kind {
            RenderNodeCommandKind::Source(RenderSourceCommand::Transparent) => {
                self.clear_target(encoder, &resources.target.view)
            }
            RenderNodeCommandKind::Source(RenderSourceCommand::Item {
                shader,
                instance,
                images,
            }) => {
                let images = self.render_images(encoder, context, images, grid)?;
                let bindings = self.image_bind_group(&images, &context.frame.transparent.view);
                let item_bindings = self.item_bind_group(
                    &context.frame.buffers.item_buffer,
                    &context.frame.buffers.properties,
                    resources
                        .uniform
                        .as_ref()
                        .expect("item nodes have a uniform"),
                );
                self.encode_item_pass(
                    encoder,
                    &resources.target.view,
                    PassBindings {
                        input: &item_bindings,
                        capabilities: &bindings,
                    },
                    shader,
                    *instance..*instance + 1,
                    wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                );
            }

            RenderNodeCommandKind::Source(RenderSourceCommand::Frame { .. }) => {
                unreachable!("native image frames are handled above")
            }
            RenderNodeCommandKind::Pass { images, pass } => {
                let images = self.render_images(encoder, context, images, grid)?;
                let bindings = self.image_bind_group(&images, &context.frame.transparent.view);
                let pass_bindings = self.pass_bind_group(
                    &context.frame.buffers.pass_properties,
                    &resources,
                    matches!(pass.kind, PassKind::Compute { .. }),
                );
                self.encode_pass_node(
                    encoder,
                    &resources,
                    PassBindings {
                        input: &pass_bindings,
                        capabilities: &bindings,
                    },
                    pass,
                )?;
            }
            RenderNodeCommandKind::Composite { children } => {
                self.clear_target(encoder, &resources.target.view);
                let request = RenderRequest {
                    bounds: grid.sampling_bounds(rect),
                    grid,
                };

                for child in children {
                    if context.plan.nodes[child.node]
                        .bounds
                        .intersection(request.bounds)
                        .is_none()
                    {
                        continue;
                    }

                    let surface = self.render_surface(encoder, context, child.node, request)?;
                    self.composite_layer(
                        encoder,
                        &resources.target,
                        context.backdrops,
                        &surface,
                        GpuComposite::new(
                            surface.size,
                            surface.rect,
                            resources.size,
                            rect,
                            RenderView::default(),
                            child.blend_mode,
                        ),
                    );
                }
            }

            RenderNodeCommandKind::View { input, view } => {
                let source_grid = grid.through_view(*view);
                let source = self.render_surface(
                    encoder,
                    context,
                    *input,
                    RenderRequest {
                        bounds: source_grid.sampling_bounds(view.source_bounds(rect)),
                        grid: source_grid,
                    },
                )?;
                self.clear_target(encoder, &resources.target.view);
                self.spatial_composite(
                    encoder,
                    &resources.target.view,
                    rect,
                    resources.size,
                    &source,
                    *view,
                );
            }
        }

        let surface = RenderedSurface {
            bounds: node.bounds,
            rect,
            size: resources.size,
            view: resources.target.view.clone(),
        };
        Ok((surface, Some(resources)))
    }

    pub(super) fn image_bind_group(
        &self,
        images: &[(ImageBinding, RenderedSurface)],
        fallback: &wgpu::TextureView,
    ) -> wgpu::BindGroup {
        let device = &self.shared.gpu.device;
        let image_size = size_of::<GpuImage>();
        let stride = image_size
            .next_multiple_of(device.limits().min_uniform_buffer_offset_alignment as usize);
        let mut metadata = vec![0_u8; stride * ImageBinding::all().len()];
        for (index, binding) in ImageBinding::all().enumerate() {
            if let Some((_, image)) = images
                .iter()
                .find(|(image_binding, _)| *image_binding == binding)
            {
                let info = GpuImage::new(image.bounds, image.rect, image.size);
                metadata[index * stride..index * stride + image_size]
                    .copy_from_slice(bytemuck::bytes_of(&info));
            }
        }

        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("zerium-image-metadata"),
            size: metadata.len() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        self.shared.gpu.queue.write_buffer(&buffer, 0, &metadata);

        let mut entries = ImageBinding::all()
            .enumerate()
            .flat_map(|(index, binding)| {
                [
                    wgpu::BindGroupEntry {
                        binding: binding.texture(),
                        resource: wgpu::BindingResource::TextureView(
                            images
                                .iter()
                                .find(|(image_binding, _)| *image_binding == binding)
                                .map_or(fallback, |(_, image)| &image.view),
                        ),
                    },
                    wgpu::BindGroupEntry {
                        binding: binding.metadata(),
                        resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                            buffer: &buffer,
                            offset: (index * stride) as u64,
                            size: std::num::NonZeroU64::new(image_size as u64),
                        }),
                    },
                ]
            })
            .collect::<Vec<_>>();

        entries.push(wgpu::BindGroupEntry {
            binding: IMAGE_SAMPLER_BINDING,
            resource: wgpu::BindingResource::Sampler(&self.shared.sampler),
        });
        self.shared
            .gpu
            .device
            .create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("zerium-image-inputs"),
                layout: &self.shared.capability_bind_group_layout,
                entries: &entries,
            })
    }

    fn render_images(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        context: &mut RenderNodeContext<'_>,
        inputs: &[ImageInput],
        grid: RasterGrid,
    ) -> Result<Vec<(ImageBinding, RenderedSurface)>, RenderError> {
        let mut images = Vec::with_capacity(inputs.len());
        for input in inputs {
            let surface = self.render_surface(
                encoder,
                context,
                input.node,
                RenderRequest {
                    bounds: context.plan.nodes[input.node].bounds,
                    grid,
                },
            )?;
            images.push((input.binding, surface));
        }

        Ok(images)
    }

    fn encode_frame(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        resources: &mut FrameResources,
        frame_images: &mut [FrameImage],
        scene: &RenderPlan,
        available_resources: Vec<NodeResources>,
    ) -> Result<FrameEncoding, RenderError> {
        let mut available_resources = available_resources;
        let mut surfaces = resources.surfaces.begin_frame();
        let mut backdrops = BackdropPool::new(std::mem::take(&mut resources.backdrops));
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

        for command in &scene.commands {
            match command {
                RenderCommand::Items(batch) => {
                    self.encode_item_pass(
                        encoder,
                        target_view,
                        PassBindings {
                            input: &resources.buffers.item_bind_group,
                            capabilities: &resources.empty_capabilities,
                        },
                        &batch.shader,
                        batch.instances.clone(),
                        wgpu::LoadOp::Load,
                    );
                }

                RenderCommand::Frame { index } => {
                    let texture = frame_images.get(*index).ok_or_else(|| {
                        RenderError::backend("video render command references a missing frame")
                    })?;
                    let bind_group = &texture.binding;
                    self.encode_frame_pass(encoder, target_view, bind_group, wgpu::LoadOp::Load);
                }

                RenderCommand::Surface {
                    layer,
                    render_scale,
                } => {
                    let mut context = RenderNodeContext {
                        frame: resources,
                        plan: scene,
                        available_resources: &mut available_resources,
                        surfaces: &mut surfaces,
                        backdrops: &mut backdrops,
                        frame_images,
                    };
                    let surface = self.render_surface(
                        encoder,
                        &mut context,
                        layer.node,
                        RenderRequest {
                            bounds: SurfaceRect::viewport(resources.composition_size),
                            grid: RasterGrid::new(
                                resources.size,
                                resources.composition_size,
                                *render_scale,
                            ),
                        },
                    )?;
                    self.composite_layer(
                        encoder,
                        &resources.scene,
                        &mut backdrops,
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
            surfaces,
            backdrops: backdrops.finish(),
            unused_resources: available_resources,
        })
    }

    pub(crate) fn submit_scene(
        &self,
        scene: &RenderScene,
        target_view: &wgpu::TextureView,
    ) -> Result<wgpu::SubmissionIndex, RenderError> {
        self.validate_scene(scene)?;
        let plan = {
            let mut identities = self
                .node_identities
                .lock()
                .map_err(|_| RenderError::backend("render node identity lock poisoned"))?;
            plan_scene(scene, &mut identities)?
        };
        let mut frame_images = self.create_frame_images(&plan.frames)?;
        let mut frame_resources = self
            .frame_resources
            .lock()
            .map_err(|_| RenderError::backend("render resource lock poisoned"))?;

        if frame_resources
            .as_ref()
            .is_none_or(|frame| !frame.satisfies(scene, &plan))
        {
            *frame_resources = Some(self.create_frame_resources(scene, &plan)?);
        }

        let frame = frame_resources
            .as_mut()
            .expect("frame resources were created");

        if !plan.items.is_empty() {
            self.shared.gpu.queue.write_buffer(
                &frame.buffers.item_buffer,
                0,
                bytemuck::cast_slice(&plan.items),
            );
        }

        if !plan.properties.is_empty() {
            self.shared
                .gpu
                .queue
                .write_buffer(&frame.buffers.properties, 0, &plan.properties);
        }

        if !plan.pass_properties.is_empty() {
            self.shared.gpu.queue.write_buffer(
                &frame.buffers.pass_properties,
                0,
                &plan.pass_properties,
            );
        }

        let mut encoder =
            self.shared
                .gpu
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
            &mut frame_images,
            &plan,
            available_local_resources,
        )?;
        self.encode_output_pass(&mut encoder, target_view, &frame.output_input);
        let submission = self.shared.gpu.queue.submit([encoder.finish()]);
        let mut reusable = scene_encoding.surfaces.finish(&mut frame.surfaces);
        reusable.extend(scene_encoding.unused_resources);
        frame.backdrops = scene_encoding.backdrops;
        self.recycle_node_resources(reusable)?;
        self.recycle_frame_images(frame_images)?;
        Ok(submission)
    }
}
