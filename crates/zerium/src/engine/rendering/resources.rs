use super::encoded_scene::{
    EncodedScene, EncodedTexture, GpuComposite, GpuCompute, GpuEffect, GpuItem, GpuTextureInput,
    RenderNodeKey,
};
use super::scene::{
    RenderEffectPassKind, RenderError, RenderItemSource, RenderScene, RenderSize, RenderView,
};
use super::surface::SurfaceRect;
use super::{FrameRenderer, PROPERTY_WORD_SIZE, SCENE_FORMAT, VIDEO_FRAME_FORMAT};
use crate::engine::frame::RgbaFrame;
use std::num::NonZeroU64;
use std::sync::Arc;
use zerium_core::timeline::BlendMode;

pub(super) struct TextureResource {
    pub(super) input_count: usize,
    pub(super) frame_target: RenderTarget,
    _uploaded_frames: Vec<Arc<UploadedVideoFrame>>,
    _input_properties: wgpu::Buffer,
    _item: wgpu::Buffer,
    _item_properties: wgpu::Buffer,
    pub(super) binding: wgpu::BindGroup,
}

pub(super) struct UploadedVideoFrame {
    pub(super) frame: Arc<RgbaFrame>,
    _texture: wgpu::Texture,
    pub(super) view: wgpu::TextureView,
}

#[derive(Default)]
pub(super) struct VideoTextureCache {
    // Adjacent project ticks can refer to the same native video frame. Keeping
    // only the previous scene avoids uploading it twice without mirroring the
    // much larger CPU frame cache in GPU memory.
    previous_scene: Vec<Arc<UploadedVideoFrame>>,
    // Per-frame transient buffers for texture items. Recreating them every
    // frame stalls the driver under GPU memory pressure, so matching shapes
    // are parked here and rewritten instead.
    scratch: Vec<ScratchTextureBuffers>,
}

pub(super) struct ScratchTextureBuffers {
    pub(super) input_count: usize,
    pub(super) property_size: usize,
    pub(super) input_properties: wgpu::Buffer,
    pub(super) item_properties: wgpu::Buffer,
    pub(super) item: wgpu::Buffer,
}

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
    fn satisfies(&self, encoded: &EncodedScene) -> bool {
        self.capacity >= encoded.items.len().max(1)
            && self.property_capacity >= encoded.properties.len().max(PROPERTY_WORD_SIZE)
    }
}

impl FrameResources {
    pub(super) fn satisfies(&self, scene: &RenderScene, encoded: &EncodedScene) -> bool {
        self.size == scene.size
            && self.composition_size == scene.composition_size
            && self.items.satisfies(encoded)
    }
}

const LOCAL_RESOURCE_CACHE_BUDGET: usize = 256 * 1024 * 1024;

impl NodeResources {
    pub(super) fn satisfies(
        &self,
        size: RenderSize,
        composition_size: RenderSize,
        encoded: &EncodedScene,
        needs_temporal: bool,
    ) -> bool {
        self.size == size
            && self.composition_size == composition_size
            && self.items.satisfies(encoded)
            && self.effect_instance_capacity >= encoded.effects.len().max(1)
            && self.effect_property_capacity
                >= encoded.effect_properties.len().max(PROPERTY_WORD_SIZE)
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

impl FrameRenderer {
    pub(super) fn recycle_local_resources(
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

    pub(super) fn recycle_texture_resources(
        &self,
        resources: Vec<TextureResource>,
    ) -> Result<(), RenderError> {
        // The queue retains submitted work, so scratch buffers can be reused next frame.
        let mut cache = self
            .video_textures
            .lock()
            .map_err(|_| RenderError::backend("video texture cache lock poisoned"))?;
        for resource in resources {
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
        Ok(())
    }

    fn create_item_resources(&self, encoded: &EncodedScene) -> Result<ItemResources, RenderError> {
        let item_capacity = encoded
            .items
            .len()
            .max(1)
            .checked_next_power_of_two()
            .ok_or_else(|| RenderError::backend("item buffer capacity overflow"))?;
        let property_capacity = encoded
            .properties
            .len()
            .max(PROPERTY_WORD_SIZE)
            .checked_next_power_of_two()
            .ok_or_else(|| RenderError::backend("property buffer capacity overflow"))?;
        let item_buffer_size = (item_capacity as u64)
            .checked_mul(size_of::<GpuItem>() as u64)
            .ok_or_else(|| RenderError::backend("item buffer size overflow"))?;
        let property_buffer_size = property_capacity as u64;
        let limits = self.shared.device.limits();
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
        let item_buffer = self.shared.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("zerium-item-buffer"),
            size: item_buffer_size,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let property_buffer = self.shared.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("zerium-item-properties-buffer"),
            size: property_buffer_size,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let bind_group = self
            .shared
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
        encoded: &EncodedScene,
    ) -> Result<FrameResources, RenderError> {
        let items = self.create_item_resources(encoded)?;
        let target = RenderTarget::new(
            &self.shared.device,
            scene.size,
            wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_SRC,
            "zerium-scene-linear-texture",
        );
        let info = self.shared.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("zerium-output-info"),
            size: size_of::<GpuComposite>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let viewport = SurfaceRect::viewport(scene.composition_size);
        self.shared.queue.write_buffer(
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
            &self.shared.device,
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
        encoded: &EncodedScene,
        needs_temporal: bool,
    ) -> Result<NodeResources, RenderError> {
        let items = self.create_item_resources(encoded)?;
        let limits = self.shared.device.limits();
        let effect_instance_stride = u64::from(limits.min_uniform_buffer_offset_alignment)
            .max(size_of::<GpuEffect>() as u64)
            .next_multiple_of(u64::from(limits.min_uniform_buffer_offset_alignment).max(1));
        let effect_instance_capacity = encoded
            .effects
            .len()
            .max(1)
            .checked_next_power_of_two()
            .ok_or_else(|| RenderError::backend("effect instance capacity overflow"))?;
        let effect_instance_buffer_size = effect_instance_stride
            .checked_mul(effect_instance_capacity as u64)
            .ok_or_else(|| RenderError::backend("effect instance buffer size overflow"))?;
        let effect_property_capacity = encoded
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
        let effect_instance_buffer = self.shared.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("zerium-effect-instance-buffer"),
            size: effect_instance_buffer_size,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let effect_property_buffer = self.shared.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("zerium-effect-properties-buffer"),
            size: effect_property_buffer_size,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let compute_info_buffer = self.shared.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("zerium-compute-info-buffer"),
            size: compute_info_buffer_size,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let targets = std::array::from_fn(|_| {
            RenderTarget::new(
                &self.shared.device,
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
            &self.shared.device,
            size,
            wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            "zerium-effect-source",
        );
        let effect_input = |label, view: &wgpu::TextureView| {
            self.shared
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
                    &self.shared.device,
                    size,
                    wgpu::TextureUsages::RENDER_ATTACHMENT
                        | wgpu::TextureUsages::TEXTURE_BINDING
                        | wgpu::TextureUsages::COPY_SRC,
                    "zerium-temporal-accumulation",
                )
            });
            let inputs = std::array::from_fn(|index| {
                self.shared
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
                && !self.shared.pipelines.contains_key(&item.shader)
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
        let limits = self.shared.device.limits();
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
                .shared
                .texture_pipelines
                .get(&encoded.shader)
                .ok_or_else(|| RenderError::backend("texture shader is not registered"))?;
            let frames = &encoded.frames;
            let input_count = frames.len();
            if input_count != pipeline.input_count {
                return Err(RenderError::backend(format!(
                    "texture input slot count for shader '{}' does not match its plugin schema",
                    encoded.shader
                )));
            }
            let mut uploaded_frames = Vec::with_capacity(input_count);
            for frame in frames {
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
            let item_property_size = encoded.properties.len().max(PROPERTY_WORD_SIZE);
            let scratch_index = cache.scratch.iter().position(|scratch| {
                scratch.input_count == input_count && scratch.property_size == item_property_size
            });
            let scratch = match scratch_index {
                Some(index) => cache.scratch.swap_remove(index),
                None => ScratchTextureBuffers {
                    input_count,
                    property_size: item_property_size,
                    input_properties: self.shared.device.create_buffer(&wgpu::BufferDescriptor {
                        label: Some("zerium-texture-input-properties"),
                        size: ((input_count + 1) * size_of::<GpuTextureInput>()) as u64,
                        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                        mapped_at_creation: false,
                    }),
                    item_properties: self.shared.device.create_buffer(&wgpu::BufferDescriptor {
                        label: Some("zerium-texture-item-properties"),
                        size: item_property_size as u64,
                        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
                        mapped_at_creation: false,
                    }),
                    item: self.shared.device.create_buffer(&wgpu::BufferDescriptor {
                        label: Some("zerium-texture-item"),
                        size: size_of::<GpuItem>() as u64,
                        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
                        mapped_at_creation: false,
                    }),
                },
            };
            let input_properties = scratch.input_properties;
            let item_properties = scratch.item_properties;
            let item = scratch.item;
            self.shared.queue.write_buffer(
                &input_properties,
                0,
                bytemuck::cast_slice(&input_metadata),
            );
            if !encoded.properties.is_empty() {
                self.shared
                    .queue
                    .write_buffer(&item_properties, 0, &encoded.properties);
            }
            self.shared.queue.write_buffer(
                &item,
                0,
                bytemuck::bytes_of(&GpuItem {
                    property_offset: 0,
                    property_size: u32::try_from(encoded.properties.len()).map_err(|_| {
                        RenderError::backend("texture item property size exceeds u32")
                    })?,
                    output_size: [
                        encoded.target_size.width as f32,
                        encoded.target_size.height as f32,
                    ],
                    composition_size: [
                        encoded.composition_size.width as f32,
                        encoded.composition_size.height as f32,
                    ],
                    surface_min: [
                        -(encoded.composition_size.width as f32) * 0.5,
                        -(encoded.composition_size.height as f32) * 0.5,
                    ],
                    surface_size: [
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
                    resource: item_properties.as_entire_binding(),
                },
            ];
            bind_entries.extend(uploaded_frames.iter().enumerate().map(|(index, uploaded)| {
                wgpu::BindGroupEntry {
                    binding: u32::try_from(index + 2).expect("texture binding index exceeds u32"),
                    resource: wgpu::BindingResource::TextureView(&uploaded.view),
                }
            }));
            let sampler_binding = u32::try_from(2 + input_count)
                .map_err(|_| RenderError::backend("too many texture inputs"))?;
            let binding = {
                bind_entries.push(wgpu::BindGroupEntry {
                    binding: sampler_binding,
                    resource: wgpu::BindingResource::Sampler(&self.shared.sampler),
                });
                bind_entries.push(wgpu::BindGroupEntry {
                    binding: sampler_binding + 1,
                    resource: input_properties.as_entire_binding(),
                });
                self.shared
                    .device
                    .create_bind_group(&wgpu::BindGroupDescriptor {
                        label: Some("zerium-texture-item-bind-group"),
                        layout: &pipeline.bind_group_layout,
                        entries: &bind_entries,
                    })
            };
            let frame = frames
                .first()
                .ok_or_else(|| RenderError::backend("capability texture has no frame"))?;
            let texture = self.shared.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("zerium-capability-frame-target"),
                size: Self::video_frame_extent(frame)?,
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: SCENE_FORMAT,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                    | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            });
            let frame_target = RenderTarget {
                view: texture.create_view(&Default::default()),
                texture,
            };
            resources.push(TextureResource {
                input_count,
                frame_target,
                _uploaded_frames: uploaded_frames,
                _input_properties: input_properties,
                _item: item,
                _item_properties: item_properties,
                binding,
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
        let texture = self.shared.device.create_texture(&wgpu::TextureDescriptor {
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
        self.shared.queue.write_texture(
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
