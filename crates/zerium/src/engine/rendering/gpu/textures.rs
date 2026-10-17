//! Consumer-local input texture preparation and scratch resource reuse.
use super::resources::RenderTarget;
use super::video_upload::YuvUpload;
use super::{INPUT_FRAME_FORMAT, RenderError, RenderSession};
use crate::engine::frame::{FrameData, PixelPlane, VideoPixels, VisualFrame};
use std::sync::Arc;

pub(super) struct FrameImage {
    pub(super) target: Option<RenderTarget>,
    pub(super) uploaded: Arc<FrameTexture>,
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
    // Conversion targets from the previous scene; unmatched sizes are dropped.
    targets: Vec<RenderTarget>,
}

impl RenderSession {
    pub(super) fn recycle_frame_images(&self, images: Vec<FrameImage>) -> Result<(), RenderError> {
        let mut cache = self
            .texture_resources
            .lock()
            .map_err(|_| RenderError::backend("video texture cache lock poisoned"))?;
        cache.targets = images
            .into_iter()
            .filter_map(|image| image.target)
            .collect();
        Ok(())
    }

    pub(super) fn create_frame_images(
        &self,
        frames: &[Arc<VisualFrame>],
    ) -> Result<Vec<FrameImage>, RenderError> {
        let mut cache = self
            .texture_resources
            .lock()
            .map_err(|_| RenderError::backend("video texture cache lock poisoned"))?;
        let mut current_scene: Vec<Arc<FrameTexture>> = Vec::with_capacity(frames.len());
        let mut images = Vec::with_capacity(frames.len());
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
                            && !frames
                                .iter()
                                .any(|desired| Arc::ptr_eq(&uploaded.frame, desired))
                    });
                    let Some(reusable) = reusable else {
                        return self.upload_frame(frame.clone(), &mut cache.yuv_upload);
                    };
                    let mut uploaded = cache.previous_scene.swap_remove(reusable);
                    let slot =
                        Arc::get_mut(&mut uploaded).expect("reusable texture has a single owner");
                    self.update_frame_texture(slot, frame.clone(), &mut cache.yuv_upload)?;
                    Ok(uploaded)
                })?;
            if !current_scene
                .iter()
                .any(|current| Arc::ptr_eq(&current.frame, frame))
            {
                current_scene.push(uploaded.clone());
            }
            let extent = Self::frame_extent(frame)?;
            let target = cache
                .targets
                .iter()
                .position(|target| target.texture.size() == extent)
                .map(|index| cache.targets.swap_remove(index));
            let binding = self
                .shared
                .gpu
                .device
                .create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("zerium-frame-image"),
                    layout: &self.shared.frame_bind_group_layout,
                    entries: &[
                        wgpu::BindGroupEntry {
                            binding: 0,
                            resource: wgpu::BindingResource::TextureView(&uploaded.view),
                        },
                        wgpu::BindGroupEntry {
                            binding: 1,
                            resource: wgpu::BindingResource::Sampler(&self.shared.sampler),
                        },
                    ],
                });
            images.push(FrameImage {
                target,
                uploaded,
                binding,
            });
        }
        cache.previous_scene = current_scene;
        Ok(images)
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
