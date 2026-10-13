use super::bindings::{GpuComposite, GpuCompute, GpuEffect, GpuItem};
use super::plan::{RenderNodeKey, RenderPlan};
use super::scene::SurfaceRect;
use super::scene::{
    RenderEffectPassKind, RenderError, RenderItemSource, RenderScene, RenderSize, RenderView,
};
use super::{PROPERTY_WORD_SIZE, RenderSession, SCENE_FORMAT};
use std::num::NonZeroU64;
use std::sync::Arc;
use zerium_core::timeline::BlendMode;

pub(super) struct ItemResources {
    capacity: usize,
    property_capacity: usize,
    pub(super) item_buffer: wgpu::Buffer,
    pub(super) property_buffer: wgpu::Buffer,
    pub(super) bind_group: wgpu::BindGroup,
}

pub(super) struct FrameResources {
    pub(super) size: RenderSize,
    pub(super) composition_size: RenderSize,
    pub(super) items: ItemResources,
    pub(super) scene: RenderTarget,
    pub(super) backdrop: Option<RenderTarget>,
    pub(super) output_input: wgpu::BindGroup,
    pub(super) empty_capabilities: wgpu::BindGroup,
    pub(super) node_caches: std::collections::HashMap<u32, Vec<CachedNode>>,
}

pub(super) struct CachedNode {
    pub(super) target: RenderTarget,
    pub(super) key: Option<Arc<RenderNodeKey>>,
}

pub(super) struct NodeResources {
    pub(super) size: RenderSize,
    composition_size: RenderSize,
    pub(super) items: ItemResources,
    pub(super) effect_instance_stride: u64,
    effect_instance_capacity: usize,
    effect_property_capacity: usize,
    pub(super) effect_instance_buffer: wgpu::Buffer,
    pub(super) effect_property_buffer: wgpu::Buffer,
    pub(super) compute_info_stride: u64,
    pub(super) compute_info_buffer: wgpu::Buffer,
    pub(super) compute_inputs: [wgpu::BindGroup; 2],
    pub(super) targets: [RenderTarget; 2],
    pub(super) source: RenderTarget,
    pub(super) effect_inputs: [wgpu::BindGroup; 2],
    pub(super) temporal: Option<TemporalResources>,
}

pub(super) struct RenderTarget {
    pub(super) texture: wgpu::Texture,
    pub(super) view: wgpu::TextureView,
}

impl RenderTarget {
    pub(super) fn new(
        device: &wgpu::Device,
        size: RenderSize,
        usage: wgpu::TextureUsages,
        label: &str,
    ) -> Self {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some(label),
            size: wgpu::Extent3d {
                width: size.width,
                height: size.height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: SCENE_FORMAT,
            usage,
            view_formats: &[],
        });
        Self {
            view: texture.create_view(&Default::default()),
            texture,
        }
    }
}

pub(super) struct TemporalResources {
    pub(super) targets: [RenderTarget; 2],
    pub(super) inputs: [wgpu::BindGroup; 2],
}

impl ItemResources {
    fn satisfies(&self, plan: &RenderPlan) -> bool {
        self.capacity >= plan.items.len().max(1)
            && self.property_capacity >= plan.properties.len().max(PROPERTY_WORD_SIZE)
    }
}

impl FrameResources {
    pub(super) fn satisfies(&self, scene: &RenderScene, plan: &RenderPlan) -> bool {
        self.size == scene.size
            && self.composition_size == scene.composition_size
            && self.items.satisfies(plan)
    }
}

const LOCAL_RESOURCE_CACHE_BUDGET: usize = 256 * 1024 * 1024;

impl NodeResources {
    pub(super) fn satisfies(
        &self,
        size: RenderSize,
        composition_size: RenderSize,
        plan: &RenderPlan,
        needs_temporal: bool,
    ) -> bool {
        self.size == size
            && self.composition_size == composition_size
            && self.items.satisfies(plan)
            && self.effect_instance_capacity >= plan.effects.len().max(1)
            && self.effect_property_capacity >= plan.effect_properties.len().max(PROPERTY_WORD_SIZE)
            && (!needs_temporal || self.temporal.is_some())
    }

    fn surface_bytes(&self) -> usize {
        let surfaces = 3_usize + usize::from(self.temporal.is_some()) * 2;
        (self.size.width as usize)
            .saturating_mul(self.size.height as usize)
            .saturating_mul(8)
            .saturating_mul(surfaces)
    }
}

impl RenderSession {
    pub(super) fn recycle_node_resources(
        &self,
        resources: Vec<NodeResources>,
    ) -> Result<(), RenderError> {
        let mut cached = self
            .local_resources
            .lock()
            .map_err(|_| RenderError::backend("local render resource lock poisoned"))?;
        let mut retained_bytes = 0_usize;
        for resource in resources {
            let bytes = resource.surface_bytes();
            if retained_bytes.saturating_add(bytes) <= LOCAL_RESOURCE_CACHE_BUDGET {
                retained_bytes += bytes;
                cached.push(resource);
            }
        }
        Ok(())
    }

    fn create_item_resources(&self, plan: &RenderPlan) -> Result<ItemResources, RenderError> {
        let item_capacity = plan
            .items
            .len()
            .max(1)
            .checked_next_power_of_two()
            .ok_or_else(|| RenderError::backend("item buffer capacity overflow"))?;
        let property_capacity = plan
            .properties
            .len()
            .max(PROPERTY_WORD_SIZE)
            .checked_next_power_of_two()
            .ok_or_else(|| RenderError::backend("property buffer capacity overflow"))?;
        let item_buffer_size = (item_capacity as u64)
            .checked_mul(size_of::<GpuItem>() as u64)
            .ok_or_else(|| RenderError::backend("item buffer size overflow"))?;
        let property_buffer_size = property_capacity as u64;
        let limits = self.shared.gpu.device.limits();
        for (name, size) in [
            ("item", item_buffer_size),
            ("property", property_buffer_size),
        ] {
            if size > limits.max_buffer_size || size > limits.max_storage_buffer_binding_size {
                return Err(RenderError::backend(format!(
                    "{name} buffer size {size} exceeds GPU limits"
                )));
            }
        }
        let item_buffer = self
            .shared
            .gpu
            .device
            .create_buffer(&wgpu::BufferDescriptor {
                label: Some("zerium-item-buffer"),
                size: item_buffer_size,
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
        let property_buffer = self
            .shared
            .gpu
            .device
            .create_buffer(&wgpu::BufferDescriptor {
                label: Some("zerium-item-properties-buffer"),
                size: property_buffer_size,
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
        let bind_group = self
            .shared
            .gpu
            .device
            .create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("zerium-item-bind-group"),
                layout: &self.shared.item_bind_group_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: item_buffer.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: property_buffer.as_entire_binding(),
                    },
                ],
            });

        Ok(ItemResources {
            capacity: item_capacity,
            property_capacity,
            item_buffer,
            property_buffer,
            bind_group,
        })
    }

    pub(super) fn create_frame_resources(
        &self,
        scene: &RenderScene,
        plan: &RenderPlan,
    ) -> Result<FrameResources, RenderError> {
        let items = self.create_item_resources(plan)?;
        let target = RenderTarget::new(
            &self.shared.gpu.device,
            scene.size,
            wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_SRC,
            "zerium-scene-linear-texture",
        );
        let info = self
            .shared
            .gpu
            .device
            .create_buffer(&wgpu::BufferDescriptor {
                label: Some("zerium-output-info"),
                size: size_of::<GpuComposite>() as u64,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
        let viewport = SurfaceRect::viewport(scene.composition_size);
        self.shared.gpu.queue.write_buffer(
            &info,
            0,
            bytemuck::bytes_of(&GpuComposite::new(
                scene.size,
                viewport,
                scene.size,
                viewport,
                RenderView::default(),
                BlendMode::Normal,
            )),
        );
        let output_input = self.composite_input_bind_group(&target.view, &info);
        let empty = RenderTarget::new(
            &self.shared.gpu.device,
            RenderSize {
                width: 1,
                height: 1,
            },
            wgpu::TextureUsages::TEXTURE_BINDING,
            "zerium-empty-input",
        );
        let empty_capabilities = self.capability_bind_group(&[], &empty.view);
        Ok(FrameResources {
            size: scene.size,
            composition_size: scene.composition_size,
            items,
            scene: target,
            backdrop: None,
            output_input,
            empty_capabilities,
            node_caches: Default::default(),
        })
    }

    pub(super) fn create_node_resources(
        &self,
        size: RenderSize,
        composition_size: RenderSize,
        plan: &RenderPlan,
        needs_temporal: bool,
    ) -> Result<NodeResources, RenderError> {
        let items = self.create_item_resources(plan)?;
        let limits = self.shared.gpu.device.limits();
        let effect_instance_stride = u64::from(limits.min_uniform_buffer_offset_alignment)
            .max(size_of::<GpuEffect>() as u64)
            .next_multiple_of(u64::from(limits.min_uniform_buffer_offset_alignment).max(1));
        let effect_instance_capacity = plan
            .effects
            .len()
            .max(1)
            .checked_next_power_of_two()
            .ok_or_else(|| RenderError::backend("effect instance capacity overflow"))?;
        let effect_instance_buffer_size = effect_instance_stride
            .checked_mul(effect_instance_capacity as u64)
            .ok_or_else(|| RenderError::backend("effect instance buffer size overflow"))?;
        let effect_property_capacity = plan
            .effect_properties
            .len()
            .max(PROPERTY_WORD_SIZE)
            .checked_next_power_of_two()
            .ok_or_else(|| RenderError::backend("effect property buffer capacity overflow"))?;
        let effect_property_buffer_size = effect_property_capacity as u64;
        let compute_info_stride = u64::from(limits.min_uniform_buffer_offset_alignment)
            .max(size_of::<GpuCompute>() as u64)
            .next_multiple_of(u64::from(limits.min_uniform_buffer_offset_alignment).max(1));
        let compute_info_buffer_size = compute_info_stride
            .checked_mul(effect_instance_capacity as u64)
            .ok_or_else(|| RenderError::backend("compute info buffer size overflow"))?;
        if effect_property_buffer_size > limits.max_buffer_size
            || effect_property_buffer_size > limits.max_storage_buffer_binding_size
        {
            return Err(RenderError::backend(format!(
                "effect property buffer size {effect_property_buffer_size} exceeds GPU limits"
            )));
        }
        let effect_instance_buffer =
            self.shared
                .gpu
                .device
                .create_buffer(&wgpu::BufferDescriptor {
                    label: Some("zerium-effect-instance-buffer"),
                    size: effect_instance_buffer_size,
                    usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                });
        let effect_property_buffer =
            self.shared
                .gpu
                .device
                .create_buffer(&wgpu::BufferDescriptor {
                    label: Some("zerium-effect-properties-buffer"),
                    size: effect_property_buffer_size,
                    usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                });
        let compute_info_buffer = self
            .shared
            .gpu
            .device
            .create_buffer(&wgpu::BufferDescriptor {
                label: Some("zerium-compute-info-buffer"),
                size: compute_info_buffer_size,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
        let targets = std::array::from_fn(|_| {
            RenderTarget::new(
                &self.shared.gpu.device,
                size,
                wgpu::TextureUsages::RENDER_ATTACHMENT
                    | wgpu::TextureUsages::TEXTURE_BINDING
                    | wgpu::TextureUsages::STORAGE_BINDING
                    | wgpu::TextureUsages::COPY_DST
                    | wgpu::TextureUsages::COPY_SRC,
                "zerium-node-target",
            )
        });
        let source = RenderTarget::new(
            &self.shared.gpu.device,
            size,
            wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            "zerium-effect-source",
        );
        let effect_input = |label, view: &wgpu::TextureView| {
            self.shared
                .gpu
                .device
                .create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some(label),
                    layout: &self.shared.effect_bind_group_layout,
                    entries: &[
                        wgpu::BindGroupEntry {
                            binding: 0,
                            resource: wgpu::BindingResource::TextureView(view),
                        },
                        wgpu::BindGroupEntry {
                            binding: 1,
                            resource: wgpu::BindingResource::Sampler(&self.shared.sampler),
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
                            resource: effect_property_buffer.as_entire_binding(),
                        },
                        wgpu::BindGroupEntry {
                            binding: 4,
                            resource: wgpu::BindingResource::TextureView(&source.view),
                        },
                    ],
                })
        };
        let effect_inputs = [
            effect_input("zerium-effect-input-a", &targets[0].view),
            effect_input("zerium-effect-input-b", &targets[1].view),
        ];
        let compute_input =
            |label, input_view: &wgpu::TextureView, output_view: &wgpu::TextureView| {
                self.shared
                    .gpu
                    .device
                    .create_bind_group(&wgpu::BindGroupDescriptor {
                        label: Some(label),
                        layout: &self.shared.compute_bind_group_layout,
                        entries: &[
                            wgpu::BindGroupEntry {
                                binding: 0,
                                resource: wgpu::BindingResource::TextureView(input_view),
                            },
                            wgpu::BindGroupEntry {
                                binding: 1,
                                resource: wgpu::BindingResource::Sampler(&self.shared.sampler),
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
                                resource: effect_property_buffer.as_entire_binding(),
                            },
                            wgpu::BindGroupEntry {
                                binding: 4,
                                resource: wgpu::BindingResource::TextureView(output_view),
                            },
                            wgpu::BindGroupEntry {
                                binding: 5,
                                resource: wgpu::BindingResource::TextureView(&source.view),
                            },
                        ],
                    })
            };
        let compute_inputs = [
            compute_input(
                "zerium-compute-input-a-output-b",
                &targets[0].view,
                &targets[1].view,
            ),
            compute_input(
                "zerium-compute-input-b-output-a",
                &targets[1].view,
                &targets[0].view,
            ),
        ];
        let temporal = needs_temporal.then(|| {
            let accumulation = std::array::from_fn(|_| {
                RenderTarget::new(
                    &self.shared.gpu.device,
                    size,
                    wgpu::TextureUsages::RENDER_ATTACHMENT
                        | wgpu::TextureUsages::TEXTURE_BINDING
                        | wgpu::TextureUsages::COPY_SRC,
                    "zerium-temporal-accumulation",
                )
            });
            let inputs = std::array::from_fn(|index| {
                self.shared
                    .gpu
                    .device
                    .create_bind_group(&wgpu::BindGroupDescriptor {
                        label: Some("zerium-temporal-input"),
                        layout: &self.shared.temporal_bind_group_layout,
                        entries: &[
                            wgpu::BindGroupEntry {
                                binding: 0,
                                resource: wgpu::BindingResource::TextureView(&targets[0].view),
                            },
                            wgpu::BindGroupEntry {
                                binding: 1,
                                resource: wgpu::BindingResource::TextureView(
                                    &accumulation[index].view,
                                ),
                            },
                            wgpu::BindGroupEntry {
                                binding: 2,
                                resource: wgpu::BindingResource::Sampler(&self.shared.sampler),
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
                                resource: effect_property_buffer.as_entire_binding(),
                            },
                        ],
                    })
            });
            TemporalResources {
                targets: accumulation,
                inputs,
            }
        });
        Ok(NodeResources {
            size,
            composition_size,
            items,
            effect_instance_stride,
            effect_instance_capacity,
            effect_property_capacity,
            effect_instance_buffer,
            effect_property_buffer,
            compute_info_stride,
            compute_info_buffer,
            compute_inputs,
            targets,
            source,
            effect_inputs,
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
            .filter(|item| matches!(item.source, RenderItemSource::Shader))
            .count();
        if item_count > u32::MAX as usize {
            return Err(RenderError::backend("too many visible items in one frame"));
        }
        if let Some(item) = render_items.iter().find(|item| {
            matches!(item.source, RenderItemSource::Shader)
                && !self.shared.item_pipelines.contains_key(&item.shader)
        }) {
            return Err(RenderError::backend(format!(
                "item shader '{}' is not registered",
                item.shader
            )));
        }
        for pass in render_effects.iter().flat_map(|effect| &effect.passes) {
            let registered = match &pass.kind {
                RenderEffectPassKind::Render => {
                    self.shared.effect_pipelines.contains_key(&pass.shader)
                }
                RenderEffectPassKind::Compute(_) => {
                    self.shared.compute_pipelines.contains_key(&pass.shader)
                }
                RenderEffectPassKind::Temporal(_) => {
                    self.shared.temporal_pipelines.contains_key(&pass.shader)
                }
            };
            if !registered {
                return Err(RenderError::backend(format!(
                    "effect shader '{}' is not registered",
                    pass.shader
                )));
            }
        }
        if let Some(item) = render_items.iter().find(|item| {
            matches!(item.source, RenderItemSource::Texture(_))
                && !self.shared.texture_pipelines.contains_key(&item.shader)
        }) {
            return Err(RenderError::backend(format!(
                "texture item shader '{}' is not registered",
                item.shader
            )));
        }
        let limits = self.shared.gpu.device.limits();
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

        if let Some(frame) = render_items.iter().find_map(|item| match &item.source {
            RenderItemSource::Texture(frames) => frames.iter().find(|frame| {
                frame.width == 0
                    || frame.height == 0
                    || frame.width > limits.max_texture_dimension_2d
                    || frame.height > limits.max_texture_dimension_2d
            }),
            RenderItemSource::Shader => None,
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
                property_offset: effect.property_offset,
                property_size: effect.property_size,
                width: size.width,
                height: size.height,
                composition_size: [
                    composition_size.width as f32,
                    composition_size.height as f32,
                ],
                surface_min: effect.surface_min,
                surface_size: effect.surface_size,
            };
            let offset = index * stride;
            bytes[offset..offset + size_of::<GpuCompute>()]
                .copy_from_slice(bytemuck::bytes_of(&info));
        }
        bytes
    }
}
