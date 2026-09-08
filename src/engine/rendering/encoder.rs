use super::*;

#[derive(Clone, Copy)]
struct RenderDepth {
    temporal: usize,
    composition: usize,
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

    pub(super) fn encode_video_pass(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        target_view: &wgpu::TextureView,
        video: &TextureResource,
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
        pass.set_bind_group(0, &video.bind_group, &[]);
        pass.draw(0..pipeline.vertex_count, 0..1);
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
        pass_command: &EffectPassCommand,
        input_is_a: bool,
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
        pass.set_bind_group(
            0,
            &resources.compute_inputs[usize::from(!input_is_a)],
            &[instance_offset],
        );
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
        mut input_is_a: bool,
    ) -> Result<bool, RenderError> {
        for effect_pass in passes {
            if effect_pass.captures_source {
                let input_texture = if input_is_a {
                    &resources.effect_texture_a
                } else {
                    &resources.effect_texture_b
                };
                encoder.copy_texture_to_texture(
                    wgpu::TexelCopyTextureInfo {
                        texture: input_texture,
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
            let (target_view, input) = if input_is_a {
                (&resources.effect_view_b, &resources.effect_input_a)
            } else {
                (&resources.effect_view_a, &resources.effect_input_b)
            };
            match effect_pass.kind {
                EffectPassCommandKind::Render => self.encode_effect_pass(
                    encoder,
                    target_view,
                    input,
                    &effect_pass.shader,
                    instance_offset,
                ),
                EffectPassCommandKind::Compute { .. } => {
                    self.encode_compute_pass(encoder, resources, effect_pass, input_is_a)?
                }
            }
            input_is_a = !input_is_a;
        }
        Ok(input_is_a)
    }

    fn encode_render_node(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        resources: &RenderResources,
        textures: &[TextureResource],
        node: &RenderNodeCommand,
        stride: u32,
        depth: RenderDepth,
    ) -> Result<bool, RenderError> {
        let temporal_depth = depth.temporal;
        let composition_depth = depth.composition;
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
                        self.encode_video_pass(
                            encoder,
                            &resources.effect_view_a,
                            texture,
                            shader,
                            wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        );
                    }
                }
                Ok(true)
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
                    match &child.kind {
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
                            self.encode_video_pass(
                                encoder,
                                &composition.view,
                                texture,
                                shader,
                                wgpu::LoadOp::Load,
                            );
                        }
                        RenderNodeCommandKind::Composite { .. }
                        | RenderNodeCommandKind::Effect { .. }
                        | RenderNodeCommandKind::TemporalEffect { .. } => {
                            let child_is_a = self.encode_render_node(
                                encoder,
                                resources,
                                textures,
                                child,
                                stride,
                                RenderDepth {
                                    temporal: temporal_depth,
                                    composition: composition_depth + 1,
                                },
                            )?;
                            let input = if child_is_a {
                                &resources.composite_input_a
                            } else {
                                &resources.composite_input_b
                            };
                            self.encode_composite_pass(encoder, &composition.view, input);
                        }
                    }
                }
                encoder.copy_texture_to_texture(
                    wgpu::TexelCopyTextureInfo {
                        texture: &composition.texture,
                        mip_level: 0,
                        origin: wgpu::Origin3d::ZERO,
                        aspect: wgpu::TextureAspect::All,
                    },
                    wgpu::TexelCopyTextureInfo {
                        texture: &resources.effect_texture_a,
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
                Ok(true)
            }
            RenderNodeCommandKind::Effect { input, passes } => {
                let input_is_a =
                    self.encode_render_node(encoder, resources, textures, input, stride, depth)?;
                self.encode_effect_passes(encoder, resources, passes, stride, input_is_a)
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
                    let input_is_a = self.encode_render_node(
                        encoder,
                        resources,
                        textures,
                        sample,
                        stride,
                        RenderDepth {
                            temporal: temporal_depth + 1,
                            composition: composition_depth,
                        },
                    )?;
                    let input_index =
                        usize::from(!input_is_a) * 2 + usize::from(!accumulation_is_a);
                    let target_view = if accumulation_is_a {
                        &temporal.view_b
                    } else {
                        &temporal.view_a
                    };
                    let instance_offset = reduce.instance.checked_mul(stride).ok_or_else(|| {
                        RenderError::backend("temporal instance offset exceeds u32")
                    })?;
                    self.encode_temporal_reduce_pass(
                        encoder,
                        target_view,
                        &temporal.inputs[input_index],
                        reduce,
                        instance_offset,
                    );
                    accumulation_is_a = !accumulation_is_a;
                }
                let accumulation_texture = if accumulation_is_a {
                    &temporal.texture_a
                } else {
                    &temporal.texture_b
                };
                encoder.copy_texture_to_texture(
                    wgpu::TexelCopyTextureInfo {
                        texture: accumulation_texture,
                        mip_level: 0,
                        origin: wgpu::Origin3d::ZERO,
                        aspect: wgpu::TextureAspect::All,
                    },
                    wgpu::TexelCopyTextureInfo {
                        texture: &resources.effect_texture_a,
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
                Ok(true)
            }
        }
    }

    pub(super) fn encode_scene(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        target_view: &wgpu::TextureView,
        resources_by_scale: &HashMap<u32, RenderResources>,
        textures: &[TextureResource],
        commands: &[RenderCommand],
        background: [f64; 4],
    ) -> Result<(), RenderError> {
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

        let base_resources = resources_by_scale
            .get(&1)
            .expect("scale-one render resources are always created");
        let stride = u32::try_from(base_resources.effect_instance_stride)
            .map_err(|_| RenderError::backend("effect instance stride exceeds u32"))?;
        for command in commands {
            match command {
                RenderCommand::Items(batch) => self.encode_item_pass(
                    encoder,
                    target_view,
                    &base_resources.bind_group,
                    &batch.shader,
                    batch.instances.clone(),
                    wgpu::LoadOp::Load,
                ),
                RenderCommand::Texture { index, shader } => {
                    let video = textures.get(*index).ok_or_else(|| {
                        RenderError::backend("video render command references a missing frame")
                    })?;
                    self.encode_video_pass(encoder, target_view, video, shader, wgpu::LoadOp::Load);
                }
                RenderCommand::Effected { node, render_scale } => {
                    let resources = resources_by_scale
                        .get(render_scale)
                        .expect("effect render resources were created for the command scale");
                    let input_is_a = self.encode_render_node(
                        encoder,
                        resources,
                        textures,
                        node,
                        stride,
                        RenderDepth::ROOT,
                    )?;
                    let composite_input = if input_is_a {
                        &resources.composite_input_a
                    } else {
                        &resources.composite_input_b
                    };
                    self.encode_composite_pass(encoder, target_view, composite_input);
                }
            }
        }
        Ok(())
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
                RenderCommand::Effected { node, .. } => Some(node.temporal_depth()),
                RenderCommand::Items(_) | RenderCommand::Texture { .. } => None,
            })
            .max()
            .unwrap_or(0);
        let composition_depth = encoded
            .commands
            .iter()
            .filter_map(|command| match command {
                RenderCommand::Effected { node, .. } => Some(node.composition_depth()),
                RenderCommand::Items(_) | RenderCommand::Texture { .. } => None,
            })
            .max()
            .unwrap_or(0);
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
                    || resources.params_capacity < encoded.params.len().max(PARAM_WORD_SIZE)
                    || resources.effect_instance_capacity < effect_pass_count.max(1)
                    || resources.effect_params_capacity
                        < encoded.effect_params.len().max(PARAM_WORD_SIZE)
                    || resources.compositions.len() < composition_depth
                    || resources.temporal.len() < temporal_depth
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
                            params_size: encoded.params.len(),
                            effect_pass_count,
                            effect_params_size: encoded.effect_params.len(),
                            composition_depth,
                            temporal_depth,
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
            if !encoded.params.is_empty() {
                self.queue
                    .write_buffer(&resources.params_buffer, 0, &encoded.params);
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
                if !encoded.effect_params.is_empty() {
                    self.queue.write_buffer(
                        &resources.effect_params_buffer,
                        0,
                        &encoded.effect_params,
                    );
                }
            }
        }

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("zerium-preview-frame-encoder"),
            });
        let base_resources = resources
            .get(&1)
            .expect("scale-one render resources were created");
        self.encode_scene(
            &mut encoder,
            &base_resources.scene_view,
            &resources,
            &texture_resources,
            &encoded.commands,
            scene.background,
        )?;
        self.encode_output_pass(&mut encoder, target_view, &base_resources.output_input);
        Ok(self.queue.submit([encoder.finish()]))
    }
}
