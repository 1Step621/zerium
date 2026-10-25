pub(in crate::engine::processing::visual::gpu) mod textures;

use super::{RenderSession, RenderState};
use crate::engine::processing::visual::{
    gpu::{
        PROPERTY_WORD_SIZE, SCENE_FORMAT,
        device::interface::{GpuComposite, GpuItem},
        plan::{execution::RenderData, surfaces::SurfaceCache},
    },
    scene::{
        RenderEffect, RenderEffectPassKind, RenderError, RenderNodeContent, RenderScene,
        RenderSize, RenderView, SurfaceRect,
    },
};
use std::collections::HashMap;
use zerium_core::timeline::BlendMode;

/// Immutable data shared by all draws in one frame.
pub(in crate::engine::processing::visual::gpu) struct FrameBuffers {
    pub item_buffer: wgpu::Buffer,
    pub properties: wgpu::Buffer,
}

pub(in crate::engine::processing::visual::gpu) struct FrameResources {
    pub(in crate::engine::processing::visual::gpu) composition_size: RenderSize,
    pub(in crate::engine::processing::visual::gpu) buffers: FrameBuffers,
    pub(in crate::engine::processing::visual::gpu) scene: RenderTarget,
    pub(in crate::engine::processing::visual::gpu) backdrops: Vec<RenderTarget>,
    pub(in crate::engine::processing::visual::gpu) output_input: wgpu::BindGroup,
    pub(in crate::engine::processing::visual::gpu) transparent: RenderTarget,
    pub(in crate::engine::processing::visual::gpu) surfaces: SurfaceCache,
}

/// Render targets and the small uniform that varies with the node's raster extent.
pub(in crate::engine::processing::visual::gpu) struct NodeResources {
    pub uniform: Option<wgpu::Buffer>,
    pub target: RenderTarget,
}

pub(in crate::engine::processing::visual::gpu) struct RenderTarget {
    pub(in crate::engine::processing::visual::gpu) texture: wgpu::Texture,
    pub(in crate::engine::processing::visual::gpu) view: wgpu::TextureView,
}

impl RenderTarget {
    pub(in crate::engine::processing::visual::gpu) fn size(&self) -> RenderSize {
        RenderSize {
            width: self.texture.width(),
            height: self.texture.height(),
        }
    }

    pub(in crate::engine::processing::visual::gpu) fn new(
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
    fn satisfies(&self, plan: &RenderData) -> bool {
        self.item_buffer.size() / size_of::<GpuItem>() as u64 >= plan.items.len().max(1) as u64
            && self.properties.size() >= plan.properties.len().max(PROPERTY_WORD_SIZE) as u64
    }
}

impl FrameResources {
    pub(in crate::engine::processing::visual::gpu) fn matches_scene(
        &self,
        scene: &RenderScene,
    ) -> bool {
        self.scene.size() == scene.size && self.composition_size == scene.composition_size
    }
}

const LOCAL_RESOURCE_CACHE_BUDGET: usize = 256 * 1024 * 1024;

pub(in crate::engine::processing::visual::gpu) fn surface_size_bytes(size: RenderSize) -> usize {
    (size.width as usize)
        .saturating_mul(size.height as usize)
        .saturating_mul(8)
}

/// Queue ordering lets all compositions of the same size share one scratch backdrop.
pub(in crate::engine::processing::visual::gpu) struct BackdropPool {
    available: Vec<RenderTarget>,
    used: HashMap<RenderSize, RenderTarget>,
}

impl BackdropPool {
    pub(in crate::engine::processing::visual::gpu) fn new(available: Vec<RenderTarget>) -> Self {
        Self {
            available,
            used: HashMap::new(),
        }
    }

    pub(in crate::engine::processing::visual::gpu) fn get(
        &mut self,
        device: &wgpu::Device,
        size: RenderSize,
    ) -> &RenderTarget {
        self.used.entry(size).or_insert_with(|| {
            if let Some(index) = self
                .available
                .iter()
                .position(|target| target.size() == size)
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

    pub(in crate::engine::processing::visual::gpu) fn finish(self) -> Vec<RenderTarget> {
        let mut remaining = LOCAL_RESOURCE_CACHE_BUDGET;
        self.used
            .into_values()
            .filter(|target| {
                let bytes = surface_size_bytes(target.size());
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
    pub(in crate::engine::processing::visual::gpu) fn satisfies(
        &self,
        size: RenderSize,
        uniform_size: u64,
    ) -> bool {
        self.target.size() == size
            && match &self.uniform {
                Some(buffer) => uniform_size > 0 && buffer.size() == uniform_size,
                None => uniform_size == 0,
            }
    }

    pub(in crate::engine::processing::visual::gpu) fn size_bytes(&self) -> usize {
        surface_size_bytes(self.target.size()).saturating_add(
            self.uniform
                .as_ref()
                .map_or(0, |buffer| buffer.size() as usize),
        )
    }
}

impl RenderState {
    pub(in crate::engine::processing::visual::gpu) fn trim_local_resources(&mut self) {
        let mut retained_bytes = 0_usize;
        self.local_resources.retain(|resource| {
            let bytes = resource.size_bytes();
            if retained_bytes.saturating_add(bytes) <= LOCAL_RESOURCE_CACHE_BUDGET {
                retained_bytes += bytes;
                true
            } else {
                false
            }
        });
    }
}

impl RenderSession {
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

    fn create_frame_buffers(&self, plan: &RenderData) -> Result<FrameBuffers, RenderError> {
        let capacity = |len: usize, minimum| {
            len.max(minimum)
                .checked_next_power_of_two()
                .ok_or_else(|| RenderError::resource_limit("frame buffer capacity overflow"))
        };
        let item_capacity = capacity(plan.items.len(), 1)?;
        let property_capacity = capacity(plan.properties.len(), PROPERTY_WORD_SIZE)?;
        let item_bytes = item_capacity
            .checked_mul(size_of::<GpuItem>())
            .ok_or_else(|| RenderError::resource_limit("item buffer size overflow"))?;
        let item_buffer = self.storage_buffer(item_bytes, "zerium-frame-items")?;
        let properties = self.storage_buffer(property_capacity, "zerium-frame-properties")?;
        Ok(FrameBuffers {
            item_buffer,
            properties,
        })
    }

    pub(in crate::engine::processing::visual::gpu) fn item_bind_group(
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

    pub(in crate::engine::processing::visual::gpu) fn pass_bind_group(
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

    pub(in crate::engine::processing::visual::gpu) fn prepare_frame_resources<'a>(
        &self,
        resources: &'a mut Option<FrameResources>,
        scene: &RenderScene,
        plan: &RenderData,
    ) -> Result<&'a mut FrameResources, RenderError> {
        if resources
            .as_ref()
            .is_none_or(|frame| !frame.matches_scene(scene))
        {
            *resources = Some(self.create_frame_resources(scene, plan)?);
        }
        let frame = resources.as_mut().expect("frame resources were created");
        // Buffer growth does not change image contents or output geometry.
        if !frame.buffers.satisfies(plan) {
            frame.buffers = self.create_frame_buffers(plan)?;
        }
        Ok(frame)
    }

    fn create_frame_resources(
        &self,
        scene: &RenderScene,
        plan: &RenderData,
    ) -> Result<FrameResources, RenderError> {
        let buffers = self.create_frame_buffers(plan)?;
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
        Ok(FrameResources {
            composition_size: scene.composition_size,
            buffers,
            scene: target,
            backdrops: Vec::new(),
            output_input,
            transparent: empty,
            surfaces: SurfaceCache::default(),
        })
    }

    pub(in crate::engine::processing::visual::gpu) fn create_node_resources(
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
        NodeResources { uniform, target }
    }

    pub(in crate::engine::processing::visual::gpu) fn validate_scene(
        &self,
        scene: &RenderScene,
    ) -> Result<(), RenderError> {
        if scene.size.width == 0 || scene.size.height == 0 {
            return Err(RenderError::backend(
                "render width and height must both be non-zero",
            ));
        }

        let limits = self.shared.gpu.device.limits();
        // Intermediate extents are validated individually by SurfaceOutput::resolve.
        // The output target is the only unconditional full-frame allocation.
        if scene.size.width > limits.max_texture_dimension_2d
            || scene.size.height > limits.max_texture_dimension_2d
        {
            return Err(RenderError::resource_limit(format!(
                "render size {}x{} exceeds GPU limit {}",
                scene.size.width, scene.size.height, limits.max_texture_dimension_2d,
            )));
        }

        let mut item_count = 0_u64;
        for node in &scene.nodes {
            let effects: &[RenderEffect] = match &node.content {
                RenderNodeContent::Item(item) => {
                    item_count += 1;
                    if item_count > u64::from(u32::MAX) {
                        return Err(RenderError::backend("too many visible items in one frame"));
                    }
                    if !self.shared.item_pipelines.contains_key(&item.shader) {
                        return Err(RenderError::backend(format!(
                            "item shader '{}' is not registered",
                            item.shader
                        )));
                    }
                    &[]
                }
                RenderNodeContent::Scene { .. } => &[],
                RenderNodeContent::Effect { effect, .. } => std::slice::from_ref(effect),
                RenderNodeContent::Frame(frame) => {
                    self.shared.frame_extent(frame)?;
                    &[]
                }
                RenderNodeContent::View { .. } => &[],
            };
            for pass in effects.iter().flat_map(|effect| &effect.passes) {
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
        }

        Ok(())
    }
}
