//! Consumer-local input texture preparation and scratch resource reuse.
use super::resources::RenderTarget;
use super::video_input::YuvUpload;
use super::{INPUT_FRAME_FORMAT, RenderDevice, RenderError, RenderSession};
use crate::engine::frame::{FrameData, PixelPlane, VideoPixels, VisualFrame};
use std::sync::{Arc, Mutex};

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

impl TextureResources {
    pub(super) fn recycle(&mut self, images: Vec<FrameImage>) {
        self.targets = images
            .into_iter()
            .filter_map(|image| image.target)
            .collect();
    }

    /// The logical DAG has already interned frames by Arc identity.
    pub(super) fn prepare(
        &mut self,
        device: &RenderDevice,
        frames: &[Arc<VisualFrame>],
    ) -> Result<Vec<FrameImage>, RenderError> {
        let mut images = Vec::with_capacity(frames.len());
        for frame in frames {
            let uploaded = self
                .previous_scene
                .iter()
                .find(|uploaded| Arc::ptr_eq(&uploaded.frame, frame))
                .cloned()
                .map(Ok)
                .unwrap_or_else(|| {
                    let reusable = self.previous_scene.iter().position(|uploaded| {
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
                        return device.upload_frame(frame.clone(), &mut self.yuv_upload);
                    };
                    let mut uploaded = self.previous_scene.swap_remove(reusable);
                    let slot =
                        Arc::get_mut(&mut uploaded).expect("reusable texture has a single owner");
                    device.update_frame_texture(slot, frame.clone(), &mut self.yuv_upload)?;
                    Ok(uploaded)
                })?;
            let extent = device.frame_extent(frame)?;
            let target = self
                .targets
                .iter()
                .position(|target| target.texture.size() == extent)
                .map(|index| self.targets.swap_remove(index));
            let binding = device
                .gpu
                .device
                .create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("zerium-frame-image"),
                    layout: &device.frame_bind_group_layout,
                    entries: &[
                        wgpu::BindGroupEntry {
                            binding: 0,
                            resource: wgpu::BindingResource::TextureView(&uploaded.view),
                        },
                        wgpu::BindGroupEntry {
                            binding: 1,
                            resource: wgpu::BindingResource::Sampler(&device.sampler),
                        },
                    ],
                });
            images.push(FrameImage {
                target,
                uploaded,
                binding,
            });
        }
        self.previous_scene = images.iter().map(|image| image.uploaded.clone()).collect();
        Ok(images)
    }
}

impl RenderSession {
    pub(crate) fn video_uploader(&self) -> VideoFrameUploader {
        VideoFrameUploader {
            shared: self.shared.clone(),
            scratch: Mutex::new(None),
        }
    }
}

/// Worker-owned conversion scratch, independent of scene planning and targets.
/// Preparing a frame includes queue submission so consumers can publish it safely.
pub(crate) struct VideoFrameUploader {
    shared: Arc<RenderDevice>,
    scratch: Mutex<Option<YuvUpload>>,
}

impl VideoFrameUploader {
    pub(crate) fn gpu(&self) -> &crate::engine::gpu::GpuContext {
        &self.shared.gpu
    }

    pub(crate) fn prepare(&self, frame: &VisualFrame) -> Result<(), RenderError> {
        let FrameData::Video(_) = frame.data else {
            return Ok(());
        };
        let mut scratch = self
            .scratch
            .lock()
            .map_err(|_| RenderError::backend("video upload scratch lock poisoned"))?;
        self.shared.upload_texture(frame, &mut scratch)?;
        // Converted pixels are consumed on this same queue. Publication needs
        // submission ordering, not a wait for GPU completion.
        self.shared.gpu.queue.submit([]);
        self.shared
            .gpu
            .device
            .poll(wgpu::PollType::Poll)
            .map_err(|error| RenderError::backend(error.to_string()))?;
        Ok(())
    }
}

impl RenderDevice {
    fn update_frame_texture(
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
        let extent = self.frame_extent(&frame)?;
        self.write_frame(&uploaded._texture, &frame, extent, yuv_upload)?;
        uploaded.frame = frame;
        Ok(())
    }

    fn upload_frame(
        &self,
        frame: Arc<VisualFrame>,
        yuv_upload: &mut Option<YuvUpload>,
    ) -> Result<Arc<FrameTexture>, RenderError> {
        let texture = self.upload_texture(&frame, yuv_upload)?;
        let view = texture.create_view(&Default::default());
        Ok(Arc::new(FrameTexture {
            frame,
            _texture: texture,
            view,
        }))
    }

    fn upload_texture(
        &self,
        frame: &VisualFrame,
        yuv_upload: &mut Option<YuvUpload>,
    ) -> Result<wgpu::Texture, RenderError> {
        let extent = self.frame_extent(frame)?;
        if let FrameData::Video(video) = &frame.data
            && let Some(texture) = video
                .rgba_texture(&self.gpu.device)
                .map_err(|e| RenderError::backend(e.to_string()))?
        {
            return Ok(texture);
        }
        let texture = self.gpu.device.create_texture(&wgpu::TextureDescriptor {
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
        self.write_frame(&texture, frame, extent, yuv_upload)?;
        Ok(texture)
    }

    fn write_frame(
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
                    .gpu_planes(&self.gpu.device)
                    .map_err(|error| RenderError::backend(error.to_string()))?
                {
                    Some((planes, format)) => self.video_planes.convert(
                        &self.gpu.device,
                        &self.gpu.queue,
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
                video.store_rgba_texture(&self.gpu.device, texture);
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
                let pipeline = &self.yuv_upload;
                if yuv_upload
                    .as_ref()
                    .is_none_or(|upload| !upload.matches(extent.width, extent.height, &format))
                {
                    *yuv_upload = Some(pipeline.create_upload(
                        &self.gpu.device,
                        extent.width,
                        extent.height,
                        &format,
                    ));
                }
                pipeline.convert(
                    &self.gpu.device,
                    &self.gpu.queue,
                    yuv_upload.as_ref().expect("YUV upload exists"),
                    &format,
                    &planes,
                    texture,
                );
                return Ok(());
            }
        };
        self.gpu.queue.write_texture(
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

    /// Validate before either worker upload or scene submission can allocate.
    pub(super) fn frame_extent(&self, frame: &VisualFrame) -> Result<wgpu::Extent3d, RenderError> {
        if frame.width == 0 || frame.height == 0 {
            return Err(RenderError::backend("frame dimensions must be non-zero"));
        }
        let limit = self.gpu.device.limits().max_texture_dimension_2d;
        if frame.width > limit || frame.height > limit {
            return Err(RenderError::resource_limit(format!(
                "frame size {}x{} exceeds GPU limit {limit}",
                frame.width, frame.height,
            )));
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
            .ok_or_else(|| RenderError::backend("frame size overflow"))?;
        if frame.byte_len() != expected_len {
            return Err(RenderError::backend(format!(
                "frame has {} bytes but {} were expected",
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
}
