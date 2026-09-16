use std::{
    collections::VecDeque,
    sync::{Arc, mpsc},
};

use super::{
    FrameRenderer, OUTPUT_FORMAT, RenderError, RenderScene, RenderSize, YUV_CONVERT,
    shader::validate_render_shader,
};

/// YUV420P planes are single-channel 8-bit targets.
const YUV_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::R8Unorm;

struct ReadbackSlot {
    // Owns the surface backing `view`; sampled through `bind_group`.
    _texture: wgpu::Texture,
    view: wgpu::TextureView,
    bind_group: wgpu::BindGroup,
    y_texture: wgpu::Texture,
    y_view: wgpu::TextureView,
    u_texture: wgpu::Texture,
    u_view: wgpu::TextureView,
    v_texture: wgpu::Texture,
    v_view: wgpu::TextureView,
    y_staging: wgpu::Buffer,
    u_staging: wgpu::Buffer,
    v_staging: wgpu::Buffer,
}

struct PendingReadback {
    frame_index: u64,
    slot: ReadbackSlot,
    submission: wgpu::SubmissionIndex,
    mapped_y: mpsc::Receiver<Result<(), wgpu::BufferAsyncError>>,
    mapped_u: mpsc::Receiver<Result<(), wgpu::BufferAsyncError>>,
    mapped_v: mpsc::Receiver<Result<(), wgpu::BufferAsyncError>>,
}

/// Keeps a small number of reusable GPU readback targets in flight.
///
/// Each frame is converted from the export surface to YUV420P on the GPU, so
/// the CPU readback transfers 1.5 bytes per pixel instead of 4 and the encoder
/// no longer needs a software RGBA to YUV conversion.
pub(crate) struct ExportFramePipeline {
    renderer: Arc<FrameRenderer>,
    size: RenderSize,
    chroma_size: RenderSize,
    extent: wgpu::Extent3d,
    chroma_extent: wgpu::Extent3d,
    y_row_bytes: u32,
    y_padded_row_bytes: u32,
    uv_row_bytes: u32,
    uv_padded_row_bytes: u32,
    y_pipeline: wgpu::RenderPipeline,
    u_pipeline: wgpu::RenderPipeline,
    v_pipeline: wgpu::RenderPipeline,
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
            .device
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("zerium-export-yuv-shader"),
                source: wgpu::ShaderSource::Wgsl(YUV_CONVERT.into()),
            });
        let bind_group_layout =
            renderer
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
                .device
                .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                    label: Some("zerium-export-yuv-pipeline-layout"),
                    bind_group_layouts: &[Some(&bind_group_layout)],
                    immediate_size: 0,
                });
        let yuv_pipeline = |entry: &str, label: &str| {
            renderer
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
        let sampler = renderer.device.create_sampler(&wgpu::SamplerDescriptor {
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
        let chroma_extent = wgpu::Extent3d {
            width: chroma_size.width,
            height: chroma_size.height,
            depth_or_array_layers: 1,
        };
        let alignment = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let y_row_bytes = size.width;
        let y_padded_row_bytes = y_row_bytes
            .div_ceil(alignment)
            .checked_mul(alignment)
            .ok_or_else(|| RenderError::backend("padded export row size is too large"))?;
        let uv_row_bytes = chroma_size.width;
        let uv_padded_row_bytes = uv_row_bytes
            .div_ceil(alignment)
            .checked_mul(alignment)
            .ok_or_else(|| RenderError::backend("padded export row size is too large"))?;
        let y_buffer_size = u64::from(y_padded_row_bytes)
            .checked_mul(u64::from(size.height))
            .ok_or_else(|| RenderError::backend("export frame is too large"))?;
        let uv_buffer_size = u64::from(uv_padded_row_bytes)
            .checked_mul(u64::from(chroma_size.height))
            .ok_or_else(|| RenderError::backend("export frame is too large"))?;
        let yuv_texture = |label: &str, plane_extent: wgpu::Extent3d| {
            renderer.device.create_texture(&wgpu::TextureDescriptor {
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
            renderer.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size: buffer_size,
                usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            })
        };
        let available = (0..depth.max(1))
            .map(|_| {
                let texture = renderer.device.create_texture(&wgpu::TextureDescriptor {
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
                let bind_group = renderer
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
                let y_texture = yuv_texture("zerium-export-y-plane", extent);
                let y_view = y_texture.create_view(&wgpu::TextureViewDescriptor::default());
                let u_texture = yuv_texture("zerium-export-u-plane", chroma_extent);
                let u_view = u_texture.create_view(&wgpu::TextureViewDescriptor::default());
                let v_texture = yuv_texture("zerium-export-v-plane", chroma_extent);
                let v_view = v_texture.create_view(&wgpu::TextureViewDescriptor::default());
                ReadbackSlot {
                    _texture: texture,
                    view,
                    bind_group,
                    y_texture,
                    y_view,
                    u_texture,
                    u_view,
                    v_texture,
                    v_view,
                    y_staging: staging_buffer("zerium-export-y-readback", y_buffer_size),
                    u_staging: staging_buffer("zerium-export-u-readback", uv_buffer_size),
                    v_staging: staging_buffer("zerium-export-v-readback", uv_buffer_size),
                }
            })
            .collect();
        Ok(Self {
            renderer,
            size,
            chroma_size,
            extent,
            chroma_extent,
            y_row_bytes,
            y_padded_row_bytes,
            uv_row_bytes,
            uv_padded_row_bytes,
            y_pipeline,
            u_pipeline,
            v_pipeline,
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
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("zerium-export-yuv-encoder"),
                });
        self.encode_plane_pass(
            &mut encoder,
            &slot.y_view,
            &slot.bind_group,
            &self.y_pipeline,
            "zerium-export-y-pass",
        );
        self.encode_plane_pass(
            &mut encoder,
            &slot.u_view,
            &slot.bind_group,
            &self.u_pipeline,
            "zerium-export-u-pass",
        );
        self.encode_plane_pass(
            &mut encoder,
            &slot.v_view,
            &slot.bind_group,
            &self.v_pipeline,
            "zerium-export-v-pass",
        );
        self.copy_plane_to_buffer(
            &mut encoder,
            &slot.y_texture,
            &slot.y_staging,
            self.extent,
            self.y_padded_row_bytes,
            self.size.height,
        );
        self.copy_plane_to_buffer(
            &mut encoder,
            &slot.u_texture,
            &slot.u_staging,
            self.chroma_extent,
            self.uv_padded_row_bytes,
            self.chroma_size.height,
        );
        self.copy_plane_to_buffer(
            &mut encoder,
            &slot.v_texture,
            &slot.v_staging,
            self.chroma_extent,
            self.uv_padded_row_bytes,
            self.chroma_size.height,
        );
        let submission = self.renderer.queue.submit([encoder.finish()]);
        let (sender_y, mapped_y) = mpsc::sync_channel(1);
        let (sender_u, mapped_u) = mpsc::sync_channel(1);
        let (sender_v, mapped_v) = mpsc::sync_channel(1);
        slot.y_staging
            .slice(..)
            .map_async(wgpu::MapMode::Read, move |result| {
                let _ = sender_y.send(result);
            });
        slot.u_staging
            .slice(..)
            .map_async(wgpu::MapMode::Read, move |result| {
                let _ = sender_u.send(result);
            });
        slot.v_staging
            .slice(..)
            .map_async(wgpu::MapMode::Read, move |result| {
                let _ = sender_v.send(result);
            });
        self.pending.push_back(PendingReadback {
            frame_index,
            slot,
            submission,
            mapped_y,
            mapped_u,
            mapped_v,
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
        texture: &wgpu::Texture,
        staging: &wgpu::Buffer,
        plane_extent: wgpu::Extent3d,
        padded_row_bytes: u32,
        rows: u32,
    ) {
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: staging,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(padded_row_bytes),
                    rows_per_image: Some(rows),
                },
            },
            plane_extent,
        );
    }

    pub(crate) fn finish_next(&mut self) -> Result<Option<(u64, Vec<u8>)>, RenderError> {
        let Some(pending) = self.pending.pop_front() else {
            return Ok(None);
        };
        self.renderer
            .device
            .poll(wgpu::PollType::Wait {
                submission_index: Some(pending.submission),
                timeout: None,
            })
            .map_err(|error| RenderError::backend(format!("GPU readback failed: {error}")))?;
        for receiver in [&pending.mapped_y, &pending.mapped_u, &pending.mapped_v] {
            receiver
                .recv()
                .map_err(|_| RenderError::backend("GPU readback callback was dropped"))?
                .map_err(|error| {
                    RenderError::backend(format!("GPU frame mapping failed: {error}"))
                })?;
        }

        let y_mapped = pending
            .slot
            .y_staging
            .slice(..)
            .get_mapped_range()
            .map_err(|error| RenderError::backend(format!("GPU frame access failed: {error}")))?;
        let u_mapped = pending
            .slot
            .u_staging
            .slice(..)
            .get_mapped_range()
            .map_err(|error| RenderError::backend(format!("GPU frame access failed: {error}")))?;
        let v_mapped = pending
            .slot
            .v_staging
            .slice(..)
            .get_mapped_range()
            .map_err(|error| RenderError::backend(format!("GPU frame access failed: {error}")))?;
        let y_len = usize::try_from(self.size.width)
            .ok()
            .and_then(|width| {
                usize::try_from(self.size.height)
                    .ok()
                    .and_then(|height| width.checked_mul(height))
            })
            .ok_or_else(|| RenderError::backend("export frame is too large"))?;
        let uv_len = usize::try_from(self.chroma_size.width)
            .ok()
            .and_then(|width| {
                usize::try_from(self.chroma_size.height)
                    .ok()
                    .and_then(|height| width.checked_mul(height))
            })
            .ok_or_else(|| RenderError::backend("export frame is too large"))?;
        let capacity = y_len
            .checked_add(uv_len)
            .and_then(|size| size.checked_add(uv_len))
            .ok_or_else(|| RenderError::backend("export frame is too large"))?;
        let mut yuv = Vec::with_capacity(capacity);
        append_depadded(
            &mut yuv,
            &y_mapped,
            usize::try_from(self.y_row_bytes)
                .map_err(|_| RenderError::backend("export row size is too large"))?,
            usize::try_from(self.y_padded_row_bytes)
                .map_err(|_| RenderError::backend("padded export row size is too large"))?,
        );
        append_depadded(
            &mut yuv,
            &u_mapped,
            usize::try_from(self.uv_row_bytes)
                .map_err(|_| RenderError::backend("export row size is too large"))?,
            usize::try_from(self.uv_padded_row_bytes)
                .map_err(|_| RenderError::backend("padded export row size is too large"))?,
        );
        append_depadded(
            &mut yuv,
            &v_mapped,
            usize::try_from(self.uv_row_bytes)
                .map_err(|_| RenderError::backend("export row size is too large"))?,
            usize::try_from(self.uv_padded_row_bytes)
                .map_err(|_| RenderError::backend("padded export row size is too large"))?,
        );
        drop(y_mapped);
        drop(u_mapped);
        drop(v_mapped);
        pending.slot.y_staging.unmap();
        pending.slot.u_staging.unmap();
        pending.slot.v_staging.unmap();
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
