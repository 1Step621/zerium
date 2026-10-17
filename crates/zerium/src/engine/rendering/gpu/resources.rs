use super::bindings::{GpuComposite, GpuItem, GpuSurface};
use super::plan::RenderPlan;
use super::scene::SurfaceRect;
use super::scene::{
    RenderEffectPassKind, RenderError, RenderNodeContent, RenderScene, RenderSize, RenderView,
};
use super::surfaces::SurfaceCache;
use super::{PROPERTY_WORD_SIZE, RenderSession, SCENE_FORMAT};
use std::collections::HashMap;
use zerium_core::timeline::BlendMode;

/// Immutable data shared by all draws in one frame.
pub(super) struct FrameBuffers {
    item_capacity: usize,
    property_capacity: usize,
    pass_property_capacity: usize,
    pub item_buffer: wgpu::Buffer,
    pub properties: wgpu::Buffer,
    pub pass_properties: wgpu::Buffer,
    pub item_bind_group: wgpu::BindGroup,
}

pub(super) struct FrameResources {
    pub(super) size: RenderSize,
    pub(super) composition_size: RenderSize,
    pub(super) buffers: FrameBuffers,
    pub(super) scene: RenderTarget,
    pub(super) backdrops: Vec<RenderTarget>,
    pub(super) output_input: wgpu::BindGroup,
    pub(super) empty_capabilities: wgpu::BindGroup,
    pub(super) transparent: RenderTarget,
    pub(super) surfaces: SurfaceCache,
}

/// Render targets and the small uniform that varies with the node's raster extent.
pub(super) struct NodeResources {
    pub size: RenderSize,
    pub uniform: Option<wgpu::Buffer>,
    pub target: RenderTarget,
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

impl FrameBuffers {
    fn satisfies(&self, plan: &RenderPlan) -> bool {
        self.item_capacity >= plan.items.len().max(1)
            && self.property_capacity >= plan.properties.len().max(PROPERTY_WORD_SIZE)
            && self.pass_property_capacity >= plan.pass_properties.len().max(PROPERTY_WORD_SIZE)
    }
}

impl FrameResources {
    pub(super) fn satisfies(&self, scene: &RenderScene, plan: &RenderPlan) -> bool {
        self.size == scene.size
            && self.composition_size == scene.composition_size
            && self.buffers.satisfies(plan)
    }
}

const LOCAL_RESOURCE_CACHE_BUDGET: usize = 256 * 1024 * 1024;

/// Queue ordering lets all compositions of the same size share one scratch backdrop.
pub(super) struct BackdropPool {
    available: Vec<RenderTarget>,
    used: HashMap<RenderSize, RenderTarget>,
}

impl BackdropPool {
    pub(super) fn new(available: Vec<RenderTarget>) -> Self {
        Self {
            available,
            used: HashMap::new(),
        }
    }

    pub(super) fn get(&mut self, device: &wgpu::Device, size: RenderSize) -> &RenderTarget {
        self.used.entry(size).or_insert_with(|| {
            let extent = wgpu::Extent3d {
                width: size.width,
                height: size.height,
                depth_or_array_layers: 1,
            };
            if let Some(index) = self
                .available
                .iter()
                .position(|target| target.texture.size() == extent)
            {
                self.available.swap_remove(index)
            } else {
                RenderTarget::new(
                    device,
                    size,
                    wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                    "zerium-composite-backdrop",
                )
            }
        })
    }

    pub(super) fn finish(self) -> Vec<RenderTarget> {
        let mut remaining = LOCAL_RESOURCE_CACHE_BUDGET;
        self.used
            .into_values()
            .filter(|target| {
                let size = target.texture.size();
                let bytes = (size.width as usize)
                    .saturating_mul(size.height as usize)
                    .saturating_mul(8);
                if bytes > remaining {
                    return false;
                }
                remaining -= bytes;
                true
            })
            .collect()
    }
}

impl NodeResources {
    pub(super) fn satisfies(&self, size: RenderSize, uniform_size: u64) -> bool {
        self.size == size
            && match &self.uniform {
                Some(buffer) => uniform_size > 0 && buffer.size() >= uniform_size,
                None => uniform_size == 0,
            }
    }

    pub(super) fn size_bytes(&self) -> usize {
        (self.size.width as usize)
            .saturating_mul(self.size.height as usize)
            .saturating_mul(8)
            .saturating_add(
                self.uniform
                    .as_ref()
                    .map_or(0, |buffer| buffer.size() as usize),
            )
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
            let bytes = resource.size_bytes();
            if retained_bytes.saturating_add(bytes) <= LOCAL_RESOURCE_CACHE_BUDGET {
                retained_bytes += bytes;
                cached.push(resource);
            }
        }
        Ok(())
    }

    fn storage_buffer(&self, size: usize, label: &str) -> Result<wgpu::Buffer, RenderError> {
        let limits = self.shared.gpu.device.limits();
        if size as u64 > limits.max_buffer_size
            || size as u64 > limits.max_storage_buffer_binding_size
        {
            return Err(RenderError::resource_limit(format!(
                "{label} exceeds GPU limits"
            )));
        }
        Ok(self
            .shared
            .gpu
            .device
            .create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size: size as u64,
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }))
    }

    fn create_frame_buffers(
        &self,
        scene: &RenderScene,
        plan: &RenderPlan,
    ) -> Result<FrameBuffers, RenderError> {
        let capacity = |len: usize, minimum| {
            len.max(minimum)
                .checked_next_power_of_two()
                .ok_or_else(|| RenderError::resource_limit("frame buffer capacity overflow"))
        };
        let item_capacity = capacity(plan.items.len(), 1)?;
        let property_capacity = capacity(plan.properties.len(), PROPERTY_WORD_SIZE)?;
        let pass_property_capacity = capacity(plan.pass_properties.len(), PROPERTY_WORD_SIZE)?;
        let item_bytes = item_capacity
            .checked_mul(size_of::<GpuItem>())
            .ok_or_else(|| RenderError::resource_limit("item buffer size overflow"))?;
        let item_buffer = self.storage_buffer(item_bytes, "zerium-frame-items")?;
        let properties = self.storage_buffer(property_capacity, "zerium-frame-item-properties")?;
        let pass_properties =
            self.storage_buffer(pass_property_capacity, "zerium-frame-pass-properties")?;
        let surface = self
            .shared
            .gpu
            .device
            .create_buffer(&wgpu::BufferDescriptor {
                label: Some("zerium-frame-surface"),
                size: size_of::<GpuSurface>() as u64,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
        let viewport = SurfaceRect::viewport(scene.composition_size);
        self.shared.gpu.queue.write_buffer(
            &surface,
            0,
            bytemuck::bytes_of(&GpuSurface {
                output_size: [scene.size.width as f32, scene.size.height as f32],
                composition_size: [
                    scene.composition_size.width as f32,
                    scene.composition_size.height as f32,
                ],
                surface_min: viewport.min.map(|value| value as f32),
                surface_size: [
                    scene.composition_size.width as f32,
                    scene.composition_size.height as f32,
                ],
            }),
        );
        let item_bind_group = self.item_bind_group(&item_buffer, &properties, &surface);
        Ok(FrameBuffers {
            item_capacity,
            property_capacity,
            pass_property_capacity,
            item_buffer,
            properties,
            pass_properties,
            item_bind_group,
        })
    }

    pub(super) fn item_bind_group(
        &self,
        items: &wgpu::Buffer,
        properties: &wgpu::Buffer,
        surface: &wgpu::Buffer,
    ) -> wgpu::BindGroup {
        self.shared
            .gpu
            .device
            .create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("zerium-item-bindings"),
                layout: &self.shared.item_bind_group_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: items.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: properties.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: surface.as_entire_binding(),
                    },
                ],
            })
    }

    pub(super) fn pass_bind_group(
        &self,
        properties: &wgpu::Buffer,
        resources: &NodeResources,
        compute: bool,
    ) -> wgpu::BindGroup {
        let mut entries = vec![
            wgpu::BindGroupEntry {
                binding: 0,
                resource: resources
                    .uniform
                    .as_ref()
                    .expect("pass nodes have a uniform")
                    .as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: properties.as_entire_binding(),
            },
        ];
        if compute {
            entries.push(wgpu::BindGroupEntry {
                binding: 2,
                resource: wgpu::BindingResource::TextureView(&resources.target.view),
            });
        }
        self.shared
            .gpu
            .device
            .create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("zerium-pass-bindings"),
                layout: if compute {
                    &self.shared.compute_bind_group_layout
                } else {
                    &self.shared.pass_bind_group_layout
                },
                entries: &entries,
            })
    }

    pub(super) fn create_frame_resources(
        &self,
        scene: &RenderScene,
        plan: &RenderPlan,
    ) -> Result<FrameResources, RenderError> {
        let buffers = self.create_frame_buffers(scene, plan)?;
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
        let empty_capabilities = self.image_bind_group(&[], &empty.view);
        Ok(FrameResources {
            size: scene.size,
            composition_size: scene.composition_size,
            buffers,
            scene: target,
            backdrops: Vec::new(),
            output_input,
            empty_capabilities,
            transparent: empty,
            surfaces: SurfaceCache::default(),
        })
    }

    pub(super) fn create_node_resources(
        &self,
        size: RenderSize,
        uniform_size: u64,
    ) -> NodeResources {
        let device = &self.shared.gpu.device;
        let uniform = (uniform_size > 0).then(|| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("zerium-node-uniform"),
                size: uniform_size,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            })
        });
        let target = RenderTarget::new(
            device,
            size,
            wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::STORAGE_BINDING
                | wgpu::TextureUsages::COPY_SRC,
            "zerium-node-target",
        );
        NodeResources {
            size,
            uniform,
            target,
        }
    }

    pub(super) fn validate_scene(&self, scene: &RenderScene) -> Result<(), RenderError> {
        if scene.size.width == 0 || scene.size.height == 0 {
            return Err(RenderError::backend(
                "render width and height must both be non-zero",
            ));
        }

        let render_items = scene.render_items();
        let render_effects = scene.render_effects();
        if render_items.len() > u32::MAX as usize {
            return Err(RenderError::backend("too many visible items in one frame"));
        }
        if let Some(item) = render_items
            .iter()
            .find(|item| !self.shared.item_pipelines.contains_key(&item.shader))
        {
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

        if let Some(frame) = scene.nodes.iter().find_map(|node| match &node.content {
            RenderNodeContent::Frame(frame)
                if frame.width == 0
                    || frame.height == 0
                    || frame.width > limits.max_texture_dimension_2d
                    || frame.height > limits.max_texture_dimension_2d =>
            {
                Some(frame)
            }
            _ => None,
        }) {
            return Err(RenderError::backend(format!(
                "video frame size {}x{} exceeds GPU limits",
                frame.width, frame.height
            )));
        }

        Ok(())
    }
}
