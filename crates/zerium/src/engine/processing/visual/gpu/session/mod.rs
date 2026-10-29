//! Mutable resources and submission for one rendering consumer.

use self::{
    passes::{PassBindings, render_pass},
    resources::{
        BackdropPool, FrameResources, NodeResources, RenderTarget,
        textures::{FrameImage, TextureResources},
    },
};
use super::{
    device::{
        RenderDevice,
        interface::{GpuComposite, GpuImage},
    },
    plan::{
        NodeIdentityCache, PassKind, build_render_graph,
        execution::{
            ExecutionCommand, ExecutionPlan, PlannedSurface, RasterOperation, SurfaceId,
            SurfaceTask,
        },
        surfaces::{RenderedSurface, SurfaceCache, SurfaceRaster},
    },
};
use crate::engine::{
    GpuContext,
    processing::visual::scene::{RenderError, RenderScene, RenderSize, RenderView, SurfaceRect},
};
use std::sync::{Arc, Mutex};
use wgpu::util::DeviceExt;
use zerium_core::timeline::BlendMode;
use zerium_shader::{IMAGE_SAMPLER_BINDING, ImageBinding};

mod passes;
mod readback;
pub(super) mod resources;
pub(crate) use self::resources::textures::VideoFrameUploader;
/// Mutable rendering session. Resource pools and uploaded-frame reuse are never
/// shared between concurrent consumers.
pub(crate) struct RenderSession {
    pub(super) shared: Arc<RenderDevice>,
    pub(super) state: Mutex<RenderState>,
}

/// Planning and submission share one ordering, including identities and pools.
#[derive(Default)]
pub(super) struct RenderState {
    frame: Option<FrameResources>,
    node_identities: NodeIdentityCache,
    local_resources: Vec<NodeResources>,
    textures: TextureResources,
}

impl RenderSession {
    pub(crate) fn gpu(&self) -> &GpuContext {
        &self.shared.gpu
    }

    pub(crate) fn fork(&self) -> Self {
        self.shared.create_session()
    }
}

struct FrameEncoding {
    surfaces: SurfaceCache,
    backdrops: Vec<RenderTarget>,
}

/// A physical task's image and allocation have one owner until the plan's final
/// consumer transfers them to either the scratch pool or the next frame's cache.
struct LiveSurface {
    surface: RenderedSurface,
    resources: Option<NodeResources>,
}

struct SurfaceContext<'a> {
    frame: &'a FrameResources,
    previous: &'a mut SurfaceCache,
    available_resources: &'a mut Vec<NodeResources>,
    backdrops: &'a mut BackdropPool,
    frame_images: &'a mut [FrameImage],
}

impl RenderSession {
    fn local_resources(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        context: &mut SurfaceContext<'_>,
        raster: SurfaceRaster,
        operation: &RasterOperation,
    ) -> NodeResources {
        let size = raster.size();
        let uniform = operation.uniform_bytes();
        let reuse = context
            .available_resources
            .iter()
            .position(|resource| resource.satisfies(size, uniform.len() as u64));
        let resources = if let Some(index) = reuse {
            context.available_resources.swap_remove(index)
        } else {
            self.create_node_resources(size, uniform.len() as u64)
        };
        if !uniform.is_empty() {
            self.write_node_uniform(
                encoder,
                resources
                    .uniform
                    .as_ref()
                    .expect("shader allocations have a uniform buffer"),
                uniform,
            );
        }
        resources
    }

    fn write_node_uniform(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        destination: &wgpu::Buffer,
        bytes: &[u8],
    ) {
        // Queue::write_buffer runs before the entire submission. A target reused
        // later in this command buffer needs its uniform update at that point.
        let upload = self
            .shared
            .gpu
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("zerium-node-uniform-upload"),
                contents: bytes,
                usage: wgpu::BufferUsages::COPY_SRC,
            });
        encoder.copy_buffer_to_buffer(&upload, 0, destination, 0, bytes.len() as u64);
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
            source.size(),
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

    fn render_planned_surface(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        context: &mut SurfaceContext<'_>,
        planned: &PlannedSurface,
        live: &[Option<LiveSurface>],
    ) -> Result<LiveSurface, RenderError> {
        match &planned.task {
            SurfaceTask::Transparent { bounds } => Ok(LiveSurface {
                surface: RenderedSurface {
                    bounds: *bounds,
                    view: context.frame.transparent.view.clone(),
                    rect: SurfaceRect {
                        min: [0.; 2],
                        max: [1.; 2],
                    },
                },
                resources: None,
            }),
            SurfaceTask::Cached { key, .. } => {
                let (surface, resources) = context
                    .previous
                    .take(*key)
                    .ok_or_else(|| RenderError::backend("planned surface cache hit is missing"))?;
                Ok(LiveSurface {
                    surface,
                    resources: Some(resources),
                })
            }
            SurfaceTask::NativeFrame { index, bounds } => Ok(LiveSurface {
                surface: self.render_native_frame(encoder, context, *index, *bounds)?,
                resources: None,
            }),
            SurfaceTask::Raster {
                bounds,
                raster,
                operation,
                ..
            } => Ok(self.encode_raster(encoder, context, *bounds, *raster, operation, live)),
        }
    }

    fn render_native_frame(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        context: &mut SurfaceContext<'_>,
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
        self.encode_frame_pass(encoder, &target.view, &texture.binding);
        Ok(RenderedSurface {
            bounds,
            rect: bounds,
            view: target.view.clone(),
        })
    }

    fn encode_raster(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        context: &mut SurfaceContext<'_>,
        bounds: SurfaceRect,
        raster: SurfaceRaster,
        operation: &RasterOperation,
        live: &[Option<LiveSurface>],
    ) -> LiveSurface {
        let rect = raster.rect();
        let resources = self.local_resources(encoder, context, raster, operation);
        match operation {
            RasterOperation::Clear => clear_target(encoder, &resources.target.view),
            RasterOperation::Item {
                shader,
                instance,
                images,
                ..
            } => {
                let bindings = self.image_bind_group(images, live, &context.frame.transparent.view);
                let item_bindings = self.item_bind_group(
                    &context.frame.buffers.item_buffer,
                    &context.frame.buffers.properties,
                    resources
                        .uniform
                        .as_ref()
                        .expect("shader allocations have a uniform buffer"),
                );
                self.encode_item_pass(
                    encoder,
                    &resources.target.view,
                    PassBindings {
                        input: &item_bindings,
                        capabilities: &bindings,
                    },
                    shader,
                    *instance,
                );
            }
            RasterOperation::Pass {
                shader,
                kind,
                images,
                ..
            } => {
                let bindings = self.image_bind_group(images, live, &context.frame.transparent.view);
                let pass_bindings = self.pass_bind_group(
                    &context.frame.buffers.properties,
                    &resources,
                    matches!(kind, PassKind::Compute { .. }),
                );
                self.encode_pass_node(
                    encoder,
                    &resources,
                    PassBindings {
                        input: &pass_bindings,
                        capabilities: &bindings,
                    },
                    shader,
                    *kind,
                );
            }
            RasterOperation::Composite { children } => {
                clear_target(encoder, &resources.target.view);
                for (blend_mode, child) in children {
                    let surface = &live[*child]
                        .as_ref()
                        .expect("composite input is live")
                        .surface;
                    self.composite_layer(
                        encoder,
                        &resources.target,
                        context.backdrops,
                        surface,
                        GpuComposite::new(
                            surface.size(),
                            surface.rect,
                            resources.target.size(),
                            rect,
                            RenderView::default(),
                            *blend_mode,
                        ),
                    );
                }
            }
            RasterOperation::View { input, view } => {
                let source = &live[*input].as_ref().expect("view input is live").surface;
                clear_target(encoder, &resources.target.view);
                self.spatial_composite(
                    encoder,
                    &resources.target.view,
                    rect,
                    resources.target.size(),
                    source,
                    *view,
                );
            }
        }
        LiveSurface {
            surface: RenderedSurface {
                bounds,
                rect,
                view: resources.target.view.clone(),
            },
            resources: Some(resources),
        }
    }

    fn image_bind_group(
        &self,
        images: &[(ImageBinding, SurfaceId)],
        live: &[Option<LiveSurface>],
        fallback: &wgpu::TextureView,
    ) -> wgpu::BindGroup {
        let device = &self.shared.gpu.device;
        let image_size = size_of::<GpuImage>();
        let stride = image_size
            .next_multiple_of(device.limits().min_uniform_buffer_offset_alignment as usize);
        let input_image = |binding| {
            images
                .iter()
                .find(|(image_binding, _)| *image_binding == binding)
                .map(|(_, id)| &live[*id].as_ref().expect("shader input is live").surface)
        };
        let mut metadata = vec![0_u8; stride * ImageBinding::all().len()];
        for (index, binding) in ImageBinding::all().enumerate() {
            if let Some(image) = input_image(binding) {
                let info = GpuImage::new(image.bounds, image.rect, image.size());
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
                            input_image(binding).map_or(fallback, |image| &image.view),
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

    fn encode_frame(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        resources: &mut FrameResources,
        frame_images: &mut [FrameImage],
        execution: &ExecutionPlan,
        available_resources: &mut Vec<NodeResources>,
    ) -> Result<FrameEncoding, RenderError> {
        let mut previous = std::mem::take(&mut resources.surfaces);
        let mut surfaces = SurfaceCache::default();
        available_resources.extend(
            previous.recycle_unrequested(
                execution
                    .surfaces
                    .iter()
                    .filter_map(PlannedSurface::cached_key),
            ),
        );
        let mut live = (0..execution.surfaces.len())
            .map(|_| None)
            .collect::<Vec<_>>();
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
            r: decode(execution.data.background[0]),
            g: decode(execution.data.background[1]),
            b: decode(execution.data.background[2]),
            a: execution.data.background[3],
        };
        render_pass(
            encoder,
            target_view,
            wgpu::LoadOp::Clear(background),
            "zerium-frame-clear-pass",
        );

        for command in &execution.commands {
            match command {
                ExecutionCommand::Render(id) => {
                    let mut context = SurfaceContext {
                        frame: resources,
                        previous: &mut previous,
                        available_resources,
                        backdrops: &mut backdrops,
                        frame_images,
                    };
                    live[*id] = Some(self.render_planned_surface(
                        encoder,
                        &mut context,
                        &execution.surfaces[*id],
                        &live,
                    )?);
                }
                ExecutionCommand::Composite { source, blend_mode } => {
                    let surface = &live[*source]
                        .as_ref()
                        .expect("root surface is live")
                        .surface;
                    self.composite_layer(
                        encoder,
                        &resources.scene,
                        &mut backdrops,
                        surface,
                        GpuComposite::new(
                            surface.size(),
                            surface.rect,
                            resources.scene.size(),
                            SurfaceRect::viewport(resources.composition_size),
                            RenderView::default(),
                            *blend_mode,
                        ),
                    );
                }
                ExecutionCommand::Release(id) => {
                    let surface = live[*id].take().expect("released surface is live");
                    if let Some(resource) = surface.resources {
                        available_resources.push(resource);
                    }
                }
                ExecutionCommand::Retain { surface: id, key } => {
                    let surface = live[*id].take().expect("retained surface is live");
                    surfaces.insert(
                        *key,
                        surface.surface,
                        surface
                            .resources
                            .expect("only allocated rasters can be retained"),
                    );
                }
            }
        }

        Ok(FrameEncoding {
            surfaces,
            backdrops: backdrops.finish(),
        })
    }

    pub(crate) fn submit_scene(
        &self,
        scene: &RenderScene,
        target_view: &wgpu::TextureView,
    ) -> Result<wgpu::SubmissionIndex, RenderError> {
        if self.gpu().is_lost() {
            return Err(RenderError::backend("GPU device was lost"));
        }
        if ![super::OUTPUT_FORMAT, super::SCENE_FORMAT].contains(&target_view.texture().format()) {
            return Err(RenderError::backend("Unsupported output surface format"));
        }
        self.validate_scene(scene)?;
        let mut state = self
            .state
            .lock()
            .map_err(|_| RenderError::backend("render state lock poisoned"))?;
        let graph = build_render_graph(scene, &mut state.node_identities)?;
        let RenderState {
            frame,
            local_resources,
            textures,
            ..
        } = &mut *state;
        let cached = frame.as_ref().filter(|frame| frame.matches_scene(scene));
        let execution = ExecutionPlan::build(
            graph,
            scene,
            self.shared.gpu.device.limits().max_texture_dimension_2d,
            |key| cached.and_then(|frame| frame.surfaces.size_bytes(key)),
        )?;
        let frame = self.prepare_frame_resources(frame, scene, &execution.data)?;
        let mut frame_images = textures.prepare(&self.shared, &execution.data.frames)?;

        if !execution.data.items.is_empty() {
            self.shared.gpu.queue.write_buffer(
                &frame.buffers.item_buffer,
                0,
                bytemuck::cast_slice(&execution.data.items),
            );
        }

        if !execution.data.properties.is_empty() {
            self.shared.gpu.queue.write_buffer(
                &frame.buffers.properties,
                0,
                &execution.data.properties,
            );
        }

        let mut encoder =
            self.shared
                .gpu
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("zerium-preview-frame-encoder"),
                });
        let scene_encoding = self.encode_frame(
            &mut encoder,
            frame,
            &mut frame_images,
            &execution,
            local_resources,
        )?;
        self.encode_output_pass(&mut encoder, target_view, &frame.output_input);
        let submission = self.shared.gpu.queue.submit([encoder.finish()]);
        frame.surfaces = scene_encoding.surfaces;
        frame.backdrops = scene_encoding.backdrops;
        textures.recycle(frame_images);
        state.trim_local_resources();
        Ok(submission)
    }
}

fn clear_target(encoder: &mut wgpu::CommandEncoder, target: &wgpu::TextureView) {
    render_pass(
        encoder,
        target,
        wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
        "zerium-surface-clear",
    );
}
