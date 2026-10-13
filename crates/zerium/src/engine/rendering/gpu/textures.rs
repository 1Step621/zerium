//! Consumer-local input texture preparation and scratch resource reuse.
use super::bindings::{GpuItem, GpuTextureInput};
use super::plan::PlannedTexture;
use super::resources::RenderTarget;
use super::video_upload::YuvUpload;
use super::{INPUT_FRAME_FORMAT, PROPERTY_WORD_SIZE, RenderError, RenderSession};
use crate::engine::frame::{FrameData, PixelPlane, VideoPixels, VisualFrame};
use std::sync::Arc;

pub(super) struct TextureResource {
    pub(super) input_count: usize,
    pub(super) frame_target: Option<RenderTarget>,
    pub(super) uploaded_frames: Vec<Arc<FrameTexture>>,
    _input_properties: wgpu::Buffer,
    _item: wgpu::Buffer,
    _item_properties: wgpu::Buffer,
    pub(super) binding: wgpu::BindGroup,
}

pub(super) struct FrameTexture {
    pub(super) frame: Arc<VisualFrame>,
    _texture: wgpu::Texture,
    pub(super) view: wgpu::TextureView,
}

#[derive(Default)]
pub(super) struct TextureResources {
    // Adjacent project ticks share uploaded views. Retained decoded frames own
    // each video's final texture.
    previous_scene: Vec<Arc<FrameTexture>>,
    // Upload planes are scratch resources; converted frames keep only their
    // final texture. Queue ordering lets consecutive frames reuse this slot.
    yuv_upload: Option<YuvUpload>,
    // Texture-item buffers and optional capability targets from the last scene.
    // Matching shapes are reused instead of allocated on every frame.
    scratch: Vec<ScratchTextureResources>,
}

pub(super) struct ScratchTextureResources {
    frame_target: Option<RenderTarget>,
    pub(super) input_count: usize,
    pub(super) property_size: usize,
    pub(super) input_properties: wgpu::Buffer,
    pub(super) item_properties: wgpu::Buffer,
    pub(super) item: wgpu::Buffer,
}

impl RenderSession {
    pub(super) fn recycle_texture_resources(
        &self,
        resources: Vec<TextureResource>,
    ) -> Result<(), RenderError> {
        // The queue retains submitted work, so scratch buffers can be reused next frame.
        let mut cache = self
            .texture_resources
            .lock()
            .map_err(|_| RenderError::backend("video texture cache lock poisoned"))?;
        // Drop unmatched resources so changing scenes cannot grow the pool.
        cache.scratch.clear();
        for resource in resources {
            cache.scratch.push(ScratchTextureResources {
                frame_target: resource.frame_target,
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

    pub(super) fn create_texture_resources(
        &self,
        textures: &[PlannedTexture],
    ) -> Result<Vec<TextureResource>, RenderError> {
        let mut cache = self
            .texture_resources
            .lock()
            .map_err(|_| RenderError::backend("video texture cache lock poisoned"))?;
        let desired_frames = textures
            .iter()
            .flat_map(|texture| texture.frames.iter())
            .collect::<Vec<_>>();
        let mut current_scene = Vec::with_capacity(textures.len());
        let mut resources = Vec::with_capacity(textures.len());
        for plan in textures {
            let pipeline = self
                .shared
                .texture_pipelines
                .get(&plan.shader)
                .ok_or_else(|| RenderError::backend("texture shader is not registered"))?;
            let frames = &plan.frames;
            let input_count = frames.len();
            if input_count != pipeline.input_count {
                return Err(RenderError::backend(format!(
                    "texture input slot count for shader '{}' does not match its plugin schema",
                    plan.shader
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
                                && !matches!(uploaded.frame.data, FrameData::Video(_))
                                && !matches!(frame.data, FrameData::Video(_))
                                && Arc::strong_count(uploaded) == 1
                                && !desired_frames
                                    .iter()
                                    .any(|desired| Arc::ptr_eq(&uploaded.frame, desired))
                        });
                        let Some(reusable) = reusable else {
                            return self.upload_frame(frame.clone(), &mut cache.yuv_upload);
                        };
                        let mut uploaded = cache.previous_scene.swap_remove(reusable);
                        let slot = Arc::get_mut(&mut uploaded)
                            .expect("reusable video texture must have a single owner");
                        self.update_frame_texture(slot, frame.clone(), &mut cache.yuv_upload)?;
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
                    plan.target_size.width as f32,
                    plan.target_size.height as f32,
                    0.,
                    0.,
                ],
            });
            let frame = frames
                .first()
                .ok_or_else(|| RenderError::backend("capability texture has no frame"))?;
            let extent = Self::frame_extent(frame)?;
            let item_property_size = plan.properties.len().max(PROPERTY_WORD_SIZE);
            let scratch_index = cache.scratch.iter().position(|scratch| {
                scratch.input_count == input_count
                    && scratch.property_size == item_property_size
                    && scratch
                        .frame_target
                        .as_ref()
                        .is_none_or(|target| target.texture.size() == extent)
            });
            let scratch = match scratch_index {
                Some(index) => cache.scratch.swap_remove(index),
                None => ScratchTextureResources {
                    frame_target: None,
                    input_count,
                    property_size: item_property_size,
                    input_properties: self.shared.gpu.device.create_buffer(
                        &wgpu::BufferDescriptor {
                            label: Some("zerium-texture-input-properties"),
                            size: ((input_count + 1) * size_of::<GpuTextureInput>()) as u64,
                            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                            mapped_at_creation: false,
                        },
                    ),
                    item_properties: self.shared.gpu.device.create_buffer(
                        &wgpu::BufferDescriptor {
                            label: Some("zerium-texture-item-properties"),
                            size: item_property_size as u64,
                            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
                            mapped_at_creation: false,
                        },
                    ),
                    item: self
                        .shared
                        .gpu
                        .device
                        .create_buffer(&wgpu::BufferDescriptor {
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
            self.shared.gpu.queue.write_buffer(
                &input_properties,
                0,
                bytemuck::cast_slice(&input_metadata),
            );
            if !plan.properties.is_empty() {
                self.shared
                    .gpu
                    .queue
                    .write_buffer(&item_properties, 0, &plan.properties);
            }
            self.shared.gpu.queue.write_buffer(
                &item,
                0,
                bytemuck::bytes_of(&GpuItem {
                    property_offset: 0,
                    property_size: u32::try_from(plan.properties.len()).map_err(|_| {
                        RenderError::backend("texture item property size exceeds u32")
                    })?,
                    output_size: [
                        plan.target_size.width as f32,
                        plan.target_size.height as f32,
                    ],
                    composition_size: [
                        plan.composition_size.width as f32,
                        plan.composition_size.height as f32,
                    ],
                    surface_min: [
                        -(plan.composition_size.width as f32) * 0.5,
                        -(plan.composition_size.height as f32) * 0.5,
                    ],
                    surface_size: [
                        plan.composition_size.width as f32,
                        plan.composition_size.height as f32,
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
                    .gpu
                    .device
                    .create_bind_group(&wgpu::BindGroupDescriptor {
                        label: Some("zerium-texture-item-bind-group"),
                        layout: &pipeline.bind_group_layout,
                        entries: &bind_entries,
                    })
            };
            resources.push(TextureResource {
                input_count,
                frame_target: scratch.frame_target,
                uploaded_frames,
                _input_properties: input_properties,
                _item: item,
                _item_properties: item_properties,
                binding,
            });
        }
        cache.previous_scene = current_scene;
        Ok(resources)
    }

    pub(crate) fn prepare_video_frame(&self, frame: &VisualFrame) -> Result<(), RenderError> {
        if !matches!(frame.data, FrameData::Video(_)) {
            return Ok(());
        }
        let mut cache = self
            .texture_resources
            .lock()
            .map_err(|_| RenderError::backend("video texture cache lock poisoned"))?;
        self.upload_frame(Arc::new(frame.clone()), &mut cache.yuv_upload)?;
        Ok(())
    }

    pub(super) fn update_frame_texture(
        &self,
        uploaded: &mut FrameTexture,
        frame: Arc<VisualFrame>,
        yuv_upload: &mut Option<YuvUpload>,
    ) -> Result<(), RenderError> {
        if uploaded.frame.width != frame.width || uploaded.frame.height != frame.height {
            return Err(RenderError::backend(
                "cannot reuse a video texture with different dimensions",
            ));
        }
        let extent = Self::frame_extent(&frame)?;
        self.write_frame(&uploaded._texture, &frame, extent, yuv_upload)?;
        uploaded.frame = frame;
        Ok(())
    }

    pub(super) fn upload_frame(
        &self,
        frame: Arc<VisualFrame>,
        yuv_upload: &mut Option<YuvUpload>,
    ) -> Result<Arc<FrameTexture>, RenderError> {
        let extent = Self::frame_extent(&frame)?;
        if let FrameData::Video(video) = &frame.data
            && let Some(texture) = video
                .rgba_texture(&self.shared.gpu.device)
                .map_err(|e| RenderError::backend(e.to_string()))?
        {
            let view = texture.create_view(&Default::default());
            return Ok(Arc::new(FrameTexture {
                frame,
                _texture: texture,
                view,
            }));
        }
        let texture = self
            .shared
            .gpu
            .device
            .create_texture(&wgpu::TextureDescriptor {
                label: Some("zerium-video-frame-texture"),
                size: extent,
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: INPUT_FRAME_FORMAT,
                usage: wgpu::TextureUsages::COPY_DST
                    | wgpu::TextureUsages::TEXTURE_BINDING
                    | wgpu::TextureUsages::RENDER_ATTACHMENT,
                view_formats: &[wgpu::TextureFormat::Rgba8Unorm],
            });
        self.write_frame(&texture, &frame, extent, yuv_upload)?;
        let view = texture.create_view(&Default::default());
        Ok(Arc::new(FrameTexture {
            frame,
            _texture: texture,
            view,
        }))
    }

    pub(super) fn frame_extent(frame: &VisualFrame) -> Result<wgpu::Extent3d, RenderError> {
        if frame.width == 0 || frame.height == 0 {
            return Err(RenderError::backend("video dimensions must be non-zero"));
        }
        if let FrameData::Video(video) = &frame.data {
            if video.dimensions() != [frame.width, frame.height] {
                return Err(RenderError::backend("Invalid decoded video dimensions"));
            }
            return Ok(wgpu::Extent3d {
                width: frame.width,
                height: frame.height,
                depth_or_array_layers: 1,
            });
        }
        let expected_len = usize::try_from(frame.width)
            .ok()
            .and_then(|width| {
                usize::try_from(frame.height)
                    .ok()
                    .and_then(|height| width.checked_mul(height))
            })
            .and_then(|pixels| pixels.checked_mul(4))
            .ok_or_else(|| RenderError::backend("video frame size overflow"))?;
        if frame.byte_len() != expected_len {
            return Err(RenderError::backend(format!(
                "video frame has {} bytes but {} were expected",
                frame.byte_len(),
                expected_len
            )));
        }
        Ok(wgpu::Extent3d {
            width: frame.width,
            height: frame.height,
            depth_or_array_layers: 1,
        })
    }

    pub(super) fn write_frame(
        &self,
        texture: &wgpu::Texture,
        frame: &VisualFrame,
        extent: wgpu::Extent3d,
        yuv_upload: &mut Option<YuvUpload>,
    ) -> Result<(), RenderError> {
        match &frame.data {
            FrameData::Rgba(bytes) => self.write_pixels(
                texture,
                extent,
                VideoPixels::Rgba(PixelPlane {
                    bytes,
                    stride: frame.width * 4,
                }),
                yuv_upload,
            )?,
            FrameData::Video(video) => {
                match video
                    .gpu_planes(&self.shared.gpu.device)
                    .map_err(|error| RenderError::backend(error.to_string()))?
                {
                    Some((planes, format)) => self.shared.video_planes.convert(
                        &self.shared.gpu.device,
                        &self.shared.gpu.queue,
                        planes,
                        format,
                        texture,
                    ),
                    None => {
                        let mapped = video
                            .map_cpu_pixels()
                            .map_err(|e| RenderError::backend(e.to_string()))?;
                        let pixels = mapped
                            .pixels()
                            .map_err(|e| RenderError::backend(e.to_string()))?;
                        self.write_pixels(texture, extent, pixels, yuv_upload)?;
                    }
                }
                video.store_rgba_texture(&self.shared.gpu.device, texture);
            }
        }
        Ok(())
    }

    fn write_pixels(
        &self,
        texture: &wgpu::Texture,
        extent: wgpu::Extent3d,
        pixels: VideoPixels<'_>,
        yuv_upload: &mut Option<YuvUpload>,
    ) -> Result<(), RenderError> {
        let plane = match pixels {
            VideoPixels::Rgba(plane) => plane,
            VideoPixels::Yuv { format, planes } => {
                let pipeline = &self.shared.yuv_upload;
                if yuv_upload
                    .as_ref()
                    .is_none_or(|upload| !upload.matches(extent.width, extent.height, &format))
                {
                    *yuv_upload = Some(pipeline.create_upload(
                        &self.shared.gpu.device,
                        extent.width,
                        extent.height,
                        &format,
                    ));
                }
                pipeline.convert(
                    &self.shared.gpu.device,
                    &self.shared.gpu.queue,
                    yuv_upload.as_ref().expect("YUV upload exists"),
                    &format,
                    &planes,
                    texture,
                );
                return Ok(());
            }
        };
        self.shared.gpu.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            plane.bytes,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(plane.stride),
                rows_per_image: Some(extent.height),
            },
            extent,
        );
        Ok(())
    }
}
