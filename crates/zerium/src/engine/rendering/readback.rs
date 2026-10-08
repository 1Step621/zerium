use zerium_shader::validate_render_shader;

use std::{
    collections::VecDeque,
    sync::{Arc, mpsc},
};

use super::{FrameRenderer, OUTPUT_FORMAT, RenderError, RenderScene, RenderSize, YUV_CONVERT};

/// YUV420P planes are single-channel 8-bit targets.
const YUV_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::R8Unorm;

struct ReadbackPlane {
    texture: wgpu::Texture,
    view: wgpu::TextureView,
    staging: wgpu::Buffer,
}

struct PlaneLayout {
    extent: wgpu::Extent3d,
    row_bytes: usize,
    padded_row_bytes: u32,
    buffer_size: u64,
    byte_len: usize,
    pipeline: wgpu::RenderPipeline,
}

impl PlaneLayout {
    fn new(size: RenderSize, pipeline: wgpu::RenderPipeline) -> Result<Self, RenderError> {
        let alignment = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let padded_row_bytes = size
            .width
            .div_ceil(alignment)
            .checked_mul(alignment)
            .ok_or_else(|| RenderError::backend("padded export row size is too large"))?;
        let row_bytes = usize::try_from(size.width)
            .map_err(|_| RenderError::backend("export row size is too large"))?;
        let byte_len = usize::try_from(size.height)
            .ok()
            .and_then(|height| row_bytes.checked_mul(height))
            .ok_or_else(|| RenderError::backend("export frame is too large"))?;
        Ok(Self {
            extent: wgpu::Extent3d {
                width: size.width,
                height: size.height,
                depth_or_array_layers: 1,
            },
            row_bytes,
            padded_row_bytes,
            buffer_size: u64::from(padded_row_bytes) * u64::from(size.height),
            byte_len,
            pipeline,
        })
    }
}

struct ReadbackSlot {
    // Owns the surface backing `view`; sampled through `bind_group`.
    _texture: wgpu::Texture,
    view: wgpu::TextureView,
    bind_group: wgpu::BindGroup,
    planes: [ReadbackPlane; 3],
}

struct PendingReadback {
    frame_index: u64,
    slot: ReadbackSlot,
    submission: wgpu::SubmissionIndex,
    mapped: [mpsc::Receiver<Result<(), wgpu::BufferAsyncError>>; 3],
}

/// Keeps a small number of reusable GPU readback targets in flight.
/// Each frame is converted to YUV420P on the GPU and read back in Y, U, V order.
pub(crate) struct ExportFramePipeline {
    renderer: Arc<FrameRenderer>,
    size: RenderSize,
    planes: [PlaneLayout; 3],
    byte_len: usize,
    available: Vec<ReadbackSlot>,
    pending: VecDeque<PendingReadback>,
}

impl ExportFramePipeline {
    pub(crate) fn new(
        renderer: Arc<FrameRenderer>,
        size: RenderSize,
        depth: usize,
    ) -> Result<Self, RenderError> {
        if size.width == 0 || size.height == 0 {
            return Err(RenderError::backend("export dimensions must be non-zero"));
        }
        if !size.width.is_multiple_of(2) || !size.height.is_multiple_of(2) {
            return Err(RenderError::backend(format!(
                "YUV420P export requires even dimensions (got {}x{})",
                size.width, size.height
            )));
        }
        // Export owns a session cache even when the caller passes the preview session.
        let renderer = Arc::new(renderer.fork());
        validate_render_shader("zerium.yuv.export-y", YUV_CONVERT, "vertex_main", "y_main")?;
        validate_render_shader("zerium.yuv.export-u", YUV_CONVERT, "vertex_main", "u_main")?;
        validate_render_shader("zerium.yuv.export-v", YUV_CONVERT, "vertex_main", "v_main")?;
        let module = renderer
            .shared
            .device
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("zerium-export-yuv-shader"),
                source: wgpu::ShaderSource::Wgsl(YUV_CONVERT.into()),
            });
        let bind_group_layout =
            renderer
                .shared
                .device
                .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                    label: Some("zerium-export-yuv-bind-group-layout"),
                    entries: &[
                        wgpu::BindGroupLayoutEntry {
                            binding: 0,
                            visibility: wgpu::ShaderStages::FRAGMENT,
                            ty: wgpu::BindingType::Texture {
                                sample_type: wgpu::TextureSampleType::Float { filterable: true },
                                view_dimension: wgpu::TextureViewDimension::D2,
                                multisampled: false,
                            },
                            count: None,
                        },
                        wgpu::BindGroupLayoutEntry {
                            binding: 1,
                            visibility: wgpu::ShaderStages::FRAGMENT,
                            ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                            count: None,
                        },
                    ],
                });
        let pipeline_layout =
            renderer
                .shared
                .device
                .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                    label: Some("zerium-export-yuv-pipeline-layout"),
                    bind_group_layouts: &[Some(&bind_group_layout)],
                    immediate_size: 0,
                });
        let yuv_pipeline = |entry: &str, label: &str| {
            renderer
                .shared
                .device
                .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                    label: Some(label),
                    layout: Some(&pipeline_layout),
                    vertex: wgpu::VertexState {
                        module: &module,
                        entry_point: Some("vertex_main"),
                        compilation_options: Default::default(),
                        buffers: &[],
                    },
                    primitive: Default::default(),
                    depth_stencil: None,
                    multisample: Default::default(),
                    fragment: Some(wgpu::FragmentState {
                        module: &module,
                        entry_point: Some(entry),
                        compilation_options: Default::default(),
                        targets: &[Some(wgpu::ColorTargetState {
                            format: YUV_FORMAT,
                            blend: None,
                            write_mask: wgpu::ColorWrites::ALL,
                        })],
                    }),
                    multiview_mask: None,
                    cache: None,
                })
        };
        let y_pipeline = yuv_pipeline("y_main", "zerium-export-y-pipeline");
        let u_pipeline = yuv_pipeline("u_main", "zerium-export-u-pipeline");
        let v_pipeline = yuv_pipeline("v_main", "zerium-export-v-pipeline");
        let sampler = renderer
            .shared
            .device
            .create_sampler(&wgpu::SamplerDescriptor {
                label: Some("zerium-export-yuv-sampler"),
                address_mode_u: wgpu::AddressMode::ClampToEdge,
                address_mode_v: wgpu::AddressMode::ClampToEdge,
                mag_filter: wgpu::FilterMode::Nearest,
                min_filter: wgpu::FilterMode::Nearest,
                mipmap_filter: wgpu::MipmapFilterMode::Nearest,
                ..Default::default()
            });

        let extent = wgpu::Extent3d {
            width: size.width,
            height: size.height,
            depth_or_array_layers: 1,
        };
        let chroma_size = RenderSize {
            width: size.width / 2,
            height: size.height / 2,
        };
        let planes = [
            PlaneLayout::new(size, y_pipeline)?,
            PlaneLayout::new(chroma_size, u_pipeline)?,
            PlaneLayout::new(chroma_size, v_pipeline)?,
        ];
        let byte_len = planes
            .iter()
            .try_fold(0_usize, |sum, plane| sum.checked_add(plane.byte_len))
            .ok_or_else(|| RenderError::backend("export frame is too large"))?;
        let yuv_texture = |label: &str, plane_extent: wgpu::Extent3d| {
            renderer
                .shared
                .device
                .create_texture(&wgpu::TextureDescriptor {
                    label: Some(label),
                    size: plane_extent,
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: YUV_FORMAT,
                    usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
                    view_formats: &[],
                })
        };
        let staging_buffer = |label: &str, buffer_size: u64| {
            renderer
                .shared
                .device
                .create_buffer(&wgpu::BufferDescriptor {
                    label: Some(label),
                    size: buffer_size,
                    usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                })
        };
        let available = (0..depth.max(1))
            .map(|_| {
                let texture = renderer
                    .shared
                    .device
                    .create_texture(&wgpu::TextureDescriptor {
                        label: Some("zerium-export-frame"),
                        size: extent,
                        mip_level_count: 1,
                        sample_count: 1,
                        dimension: wgpu::TextureDimension::D2,
                        format: OUTPUT_FORMAT,
                        usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                            | wgpu::TextureUsages::TEXTURE_BINDING,
                        view_formats: &[],
                    });
                let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
                let bind_group =
                    renderer
                        .shared
                        .device
                        .create_bind_group(&wgpu::BindGroupDescriptor {
                            label: Some("zerium-export-yuv-bind-group"),
                            layout: &bind_group_layout,
                            entries: &[
                                wgpu::BindGroupEntry {
                                    binding: 0,
                                    resource: wgpu::BindingResource::TextureView(&view),
                                },
                                wgpu::BindGroupEntry {
                                    binding: 1,
                                    resource: wgpu::BindingResource::Sampler(&sampler),
                                },
                            ],
                        });
                let planes = std::array::from_fn(|index| {
                    let layout = &planes[index];
                    let texture = yuv_texture("zerium-export-yuv-plane", layout.extent);
                    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
                    ReadbackPlane {
                        texture,
                        view,
                        staging: staging_buffer("zerium-export-yuv-readback", layout.buffer_size),
                    }
                });
                ReadbackSlot {
                    _texture: texture,
                    view,
                    bind_group,
                    planes,
                }
            })
            .collect();
        Ok(Self {
            renderer,
            size,
            planes,
            byte_len,
            available,
            pending: VecDeque::with_capacity(depth.max(1)),
        })
    }

    /// Submits one frame and returns the oldest frame when all slots are occupied.
    /// The returned bytes are tightly packed YUV420P (Y, then U, then V).
    pub(crate) fn submit(
        &mut self,
        frame_index: u64,
        scene: &RenderScene,
    ) -> Result<Option<(u64, Vec<u8>)>, RenderError> {
        if scene.size != self.size {
            return Err(RenderError::backend(
                "export scene dimensions changed during rendering",
            ));
        }
        let ready = if self.available.is_empty() {
            self.finish_next()?
        } else {
            None
        };
        let slot = self
            .available
            .pop()
            .expect("a completed readback made one slot available");
        // Queued after this submit on the same queue, so the conversion below
        // observes this frame's render.
        self.renderer.render_to_view(scene, &slot.view)?;
        let mut encoder =
            self.renderer
                .shared
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("zerium-export-yuv-encoder"),
                });
        for (plane, layout) in slot.planes.iter().zip(&self.planes) {
            self.encode_plane_pass(
                &mut encoder,
                &plane.view,
                &slot.bind_group,
                &layout.pipeline,
                "zerium-export-yuv-pass",
            );
            self.copy_plane_to_buffer(&mut encoder, plane, layout);
        }
        let submission = self.renderer.shared.queue.submit([encoder.finish()]);
        let mapped = std::array::from_fn(|index| {
            let (sender, receiver) = mpsc::sync_channel(1);
            slot.planes[index]
                .staging
                .slice(..)
                .map_async(wgpu::MapMode::Read, move |result| {
                    let _ = sender.send(result);
                });
            receiver
        });
        self.pending.push_back(PendingReadback {
            frame_index,
            slot,
            submission,
            mapped,
        });
        Ok(ready)
    }

    fn encode_plane_pass(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        target_view: &wgpu::TextureView,
        bind_group: &wgpu::BindGroup,
        pipeline: &wgpu::RenderPipeline,
        label: &str,
    ) {
        let color_attachments = [Some(wgpu::RenderPassColorAttachment {
            view: target_view,
            depth_slice: None,
            resolve_target: None,
            ops: wgpu::Operations {
                load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                store: wgpu::StoreOp::Store,
            },
        })];
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some(label),
            color_attachments: &color_attachments,
            ..Default::default()
        });
        pass.set_pipeline(pipeline);
        pass.set_bind_group(0, bind_group, &[]);
        pass.draw(0..3, 0..1);
    }

    fn copy_plane_to_buffer(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        plane: &ReadbackPlane,
        layout: &PlaneLayout,
    ) {
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &plane.texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &plane.staging,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(layout.padded_row_bytes),
                    rows_per_image: Some(layout.extent.height),
                },
            },
            layout.extent,
        );
    }

    pub(crate) fn finish_next(&mut self) -> Result<Option<(u64, Vec<u8>)>, RenderError> {
        let Some(pending) = self.pending.pop_front() else {
            return Ok(None);
        };
        self.renderer
            .shared
            .device
            .poll(wgpu::PollType::Wait {
                submission_index: Some(pending.submission),
                timeout: None,
            })
            .map_err(|error| RenderError::backend(format!("GPU readback failed: {error}")))?;
        for receiver in &pending.mapped {
            receiver
                .recv()
                .map_err(|_| RenderError::backend("GPU readback callback was dropped"))?
                .map_err(|error| {
                    RenderError::backend(format!("GPU frame mapping failed: {error}"))
                })?;
        }

        let mut yuv = Vec::with_capacity(self.byte_len);
        for (plane, layout) in pending.slot.planes.iter().zip(&self.planes) {
            let mapped = plane
                .staging
                .slice(..)
                .get_mapped_range()
                .map_err(|error| {
                    RenderError::backend(format!("GPU frame access failed: {error}"))
                })?;
            append_depadded(
                &mut yuv,
                &mapped,
                layout.row_bytes,
                layout.padded_row_bytes as usize,
            );
            drop(mapped);
            plane.staging.unmap();
        }
        let frame_index = pending.frame_index;
        self.available.push(pending.slot);
        Ok(Some((frame_index, yuv)))
    }
}

fn append_depadded(target: &mut Vec<u8>, mapped: &[u8], row_bytes: usize, padded_row_bytes: usize) {
    for row in mapped.chunks_exact(padded_row_bytes) {
        target.extend_from_slice(&row[..row_bytes]);
    }
}
