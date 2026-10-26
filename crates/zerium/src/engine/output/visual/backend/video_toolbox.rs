use std::collections::VecDeque;
use zerium_core::timeline::{ExportColorRange, VideoCodec};

use apple_cf::{
    cf::{CFDictionary, CFString},
    cm::CMTime,
    cv::{CVPixelBuffer, CVPixelBufferPool},
};
use objc2_io_surface::IOSurfaceRef;
use objc2_metal::{
    MTLDevice, MTLPixelFormat, MTLTextureDescriptor, MTLTextureType, MTLTextureUsage,
};
use videotoolbox::{
    compression::{CompressionSession, EncoderGpu, HardwareAcceleration, ProfileLevel},
    session::Codec,
};

use crate::engine::{
    output::{
        VideoPixelFormat,
        visual::{
            Cancellation, ColorConversion, EncodingError, EncodingSettings, PendingFrame,
            VideoEncoderFrame, plane_formats, wait_for_submission,
        },
    },
    processing::visual::{RenderScene, RenderSession},
};

struct Surface {
    pixel: CVPixelBuffer,
    textures: Vec<wgpu::Texture>,
}

impl Surface {
    fn views(&self) -> Vec<wgpu::TextureView> {
        self.textures
            .iter()
            .map(|texture| texture.create_view(&Default::default()))
            .collect()
    }
}

pub(in crate::engine::output::visual) struct Encoder {
    session: CompressionSession,
    free: Vec<Surface>,
    settings: EncodingSettings,
    pending: VecDeque<PendingFrame<Surface>>,
}

impl Encoder {
    pub(in crate::engine::output::visual) fn new(
        device: &wgpu::Device,
        settings: EncodingSettings,
        depth: usize,
    ) -> Result<Option<Self>, EncodingError> {
        let Some(hal) = (unsafe { device.as_hal::<wgpu::hal::api::Metal>() }) else {
            return Ok(None);
        };
        let (codec, profile) = match settings.video_codec {
            VideoCodec::H264 => (Codec::H264, ProfileLevel::H264HighAutoLevel),
            VideoCodec::H265 => (Codec::HEVC, ProfileLevel::HEVCMainAutoLevel),
            VideoCodec::Vp9 | VideoCodec::Av1 => return Ok(None),
        };
        let output = settings.output;
        let builder = CompressionSession::builder(output.width as i32, output.height as i32, codec)
            .with_hardware_acceleration(HardwareAcceleration::Required)
            .with_encoder_gpu(EncoderGpu::Required(hal.raw_device().registryID()))
            .with_allow_frame_reordering(false)
            .with_expected_frame_rate(
                f64::from(settings.frame_rate.numerator())
                    / f64::from(settings.frame_rate.denominator()),
            )
            .with_max_keyframe_interval(settings.gop.min(i32::MAX as u32) as i32)
            .with_profile_level(profile);
        let builder = if let Some(bitrate) = settings.video_bitrate_kbps {
            builder.with_average_bit_rate((bitrate * 1000) as i32)
        } else {
            builder.with_quality(0.9)
        };
        let session = builder.build().map_err(EncodingError::backend)?;
        let keys = [
            CFString::new("ColorPrimaries"),
            CFString::new("TransferFunction"),
            CFString::new("YCbCrMatrix"),
        ];
        let bt709 = CFString::new("ITU_R_709_2");
        session
            .set_properties(&CFDictionary::from_pairs(&[
                (&keys[0], &bt709),
                (&keys[1], &bt709),
                (&keys[2], &bt709),
            ]))
            .map_err(EncodingError::backend)?;
        let pool = CVPixelBufferPool::create(
            output.width as usize,
            output.height as usize,
            u32::from_be_bytes(if settings.color.range == ExportColorRange::Full {
                *b"420f"
            } else {
                *b"420v"
            }),
            depth,
        )
        .map_err(EncodingError::backend)?;
        // Retain a fixed set of pixel buffers. Cache identity and reuse never
        // depend on raw addresses or CoreVideo's pool eviction policy.
        let mut free = Vec::with_capacity(depth);
        for _ in 0..depth {
            let pixel = pool.create_pixel_buffer().map_err(EncodingError::backend)?;
            let surface = pixel
                .io_surface()
                .ok_or(EncodingError::Invalid("CoreVideo frame has no IOSurface"))?;
            // SAFETY: both libraries wrap the public IOSurfaceRef type. `surface`
            // owns the reference for this borrow; Metal retains it in each texture.
            let surface = unsafe { &*surface.as_ptr().cast::<IOSurfaceRef>() };
            let mut textures = Vec::with_capacity(2);
            for (plane, format) in plane_formats(VideoPixelFormat::Nv12).iter().enumerate() {
                let (width, height, metal_format) = if plane == 0 {
                    (output.width, output.height, MTLPixelFormat::R8Unorm)
                } else {
                    (
                        output.width / 2,
                        output.height / 2,
                        MTLPixelFormat::RG8Unorm,
                    )
                };
                let descriptor = MTLTextureDescriptor::new();
                descriptor.setTextureType(MTLTextureType::Type2D);
                descriptor.setPixelFormat(metal_format);
                descriptor.setUsage(MTLTextureUsage::RenderTarget);
                unsafe {
                    descriptor.setWidth(width as usize);
                    descriptor.setHeight(height as usize);
                }
                let raw = hal
                    .raw_device()
                    .newTextureWithDescriptor_iosurface_plane(&descriptor, surface, plane)
                    .ok_or(EncodingError::Invalid(
                        "Could not create the Metal export plane",
                    ))?;
                let size = wgpu::Extent3d {
                    width,
                    height,
                    depth_or_array_layers: 1,
                };
                let desc = wgpu::TextureDescriptor {
                    label: Some("zerium-export-iosurface"),
                    size,
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: *format,
                    usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                    view_formats: &[],
                };
                // SAFETY: the descriptor matches the IOSurface plane and its
                // Metal texture. wgpu retains the texture; `pixel` owns its slot.
                let texture = unsafe {
                    let texture = wgpu::hal::metal::Device::texture_from_raw(
                        raw,
                        *format,
                        MTLTextureType::Type2D,
                        1,
                        1,
                        wgpu::hal::CopyExtent {
                            width,
                            height,
                            depth: 1,
                        },
                        None,
                    );
                    device.create_texture_from_hal::<wgpu::hal::api::Metal>(
                        texture,
                        &desc,
                        wgpu::TextureUses::COLOR_TARGET,
                    )
                };
                textures.push(texture);
            }
            free.push(Surface { pixel, textures });
        }
        Ok(Some(Self {
            session,
            free,
            settings,
            pending: VecDeque::with_capacity(depth),
        }))
    }

    pub(in crate::engine::output::visual) fn submit(
        &mut self,
        index: u64,
        renderer: &RenderSession,
        conversion: &ColorConversion,
        scene: &RenderScene,
    ) -> Result<(), EncodingError> {
        let surface = self
            .free
            .pop()
            .ok_or(EncodingError::Invalid("No free export surface"))?;
        let commands = conversion.render(renderer, scene, &surface.views())?;
        let retained = surface.pixel.clone();
        let queue = &renderer.gpu().queue;
        let submission = queue.submit([commands.finish()]);
        queue.on_submitted_work_done(move || drop(retained));
        self.pending.push_back(PendingFrame {
            index,
            data: surface,
            submission,
        });
        Ok(())
    }

    pub(in crate::engine::output::visual) fn pending_frames(&self) -> usize {
        self.pending.len()
    }

    pub(in crate::engine::output::visual) fn finish_next(
        &mut self,
        device: &wgpu::Device,
        cancelled: Cancellation<'_>,
    ) -> Result<Option<VideoEncoderFrame>, EncodingError> {
        let Some(pending) = self.pending.pop_front() else {
            return Ok(None);
        };
        wait_for_submission(device, pending.submission, cancelled)?;
        self.encode(pending.data, pending.index).map(Some)
    }

    fn encode(&mut self, surface: Surface, index: u64) -> Result<VideoEncoderFrame, EncodingError> {
        let rate = self.settings.frame_rate;
        let value = index
            .checked_mul(u64::from(rate.denominator()))
            .and_then(|value| i64::try_from(value).ok())
            .ok_or(EncodingError::Invalid("Video timestamp is too large"))?;
        let scale = i32::try_from(rate.numerator()).map_err(EncodingError::backend)?;
        let image = surface
            .pixel
            .io_surface()
            .ok_or(EncodingError::Invalid("CoreVideo frame has no IOSurface"))?;
        let encoded = self
            .session
            .encode(&image, CMTime::new(value, scale))
            .map_err(EncodingError::backend)?;
        let sample = encoded.cm_sample_buffer().ok_or(EncodingError::Invalid(
            "VideoToolbox dropped an export frame",
        ))?;
        let parameters = sample
            .format_description()
            .ok_or(EncodingError::Invalid(
                "Encoded frame has no format description",
            ))?
            .video_parameter_sets()
            .map_err(EncodingError::backend)?;
        let length =
            usize::try_from(parameters.nal_unit_header_length).map_err(EncodingError::backend)?;
        if !(1..=4).contains(&length) {
            return Err(EncodingError::Invalid("Invalid encoded NAL length"));
        }
        let keyframe = sample.is_sync_sample();
        let mut data = Vec::with_capacity(encoded.data.len() + 128);
        if keyframe {
            for parameter in parameters.parameter_sets {
                data.extend_from_slice(&[0, 0, 0, 1]);
                data.extend(parameter);
            }
        }
        let mut input = encoded.data.as_slice();
        while !input.is_empty() {
            let prefix = input
                .get(..length)
                .ok_or(EncodingError::Invalid("Truncated encoded NAL length"))?;
            let size = prefix
                .iter()
                .fold(0usize, |size, byte| (size << 8) | usize::from(*byte));
            input = &input[length..];
            let nal = input
                .get(..size)
                .ok_or(EncodingError::Invalid("Truncated encoded NAL unit"))?;
            data.extend_from_slice(&[0, 0, 0, 1]);
            data.extend_from_slice(nal);
            input = &input[size..];
        }
        self.free.push(surface);
        Ok(VideoEncoderFrame {
            index,
            data,
            keyframe,
        })
    }
}
