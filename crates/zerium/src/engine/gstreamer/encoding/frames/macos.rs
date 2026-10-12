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

use super::{EncoderError, ExportFrame, VideoEncoderSettings};

pub(super) struct Surface {
    pub(super) pixel: CVPixelBuffer,
    textures: Vec<wgpu::Texture>,
}

impl Surface {
    pub(super) fn views(&self) -> Vec<wgpu::TextureView> {
        self.textures
            .iter()
            .map(|texture| texture.create_view(&Default::default()))
            .collect()
    }
}

pub(super) struct Encoder {
    session: CompressionSession,
    free: Vec<Surface>,
    settings: VideoEncoderSettings,
}

impl Encoder {
    pub(super) fn new(
        device: &wgpu::Device,
        settings: VideoEncoderSettings,
        depth: usize,
    ) -> Result<Option<Self>, EncoderError> {
        let Some(hal) = (unsafe { device.as_hal::<wgpu::hal::api::Metal>() }) else {
            return Ok(None);
        };
        let output = settings.output;
        let session =
            CompressionSession::builder(output.width as i32, output.height as i32, Codec::H264)
                .with_hardware_acceleration(HardwareAcceleration::Required)
                .with_encoder_gpu(EncoderGpu::Required(hal.raw_device().registryID()))
                .with_allow_frame_reordering(false)
                .with_expected_frame_rate(
                    f64::from(settings.frame_rate.numerator())
                        / f64::from(settings.frame_rate.denominator()),
                )
                .with_max_keyframe_interval(settings.gop.min(i32::MAX as u32) as i32)
                .with_quality(0.9)
                .with_profile_level(ProfileLevel::H264HighAutoLevel)
                .build()
                .map_err(EncoderError::pipeline)?;
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
            .map_err(EncoderError::pipeline)?;
        let pool = CVPixelBufferPool::create(
            output.width as usize,
            output.height as usize,
            u32::from_be_bytes(*b"420v"),
            depth,
        )
        .map_err(EncoderError::pipeline)?;
        // Retain a fixed set of pixel buffers. Cache identity and reuse never
        // depend on raw addresses or CoreVideo's pool eviction policy.
        let mut free = Vec::with_capacity(depth);
        for _ in 0..depth {
            let pixel = pool.create_pixel_buffer().map_err(EncoderError::pipeline)?;
            let surface = pixel
                .io_surface()
                .ok_or(EncoderError::Invalid("CoreVideo frame has no IOSurface"))?;
            // SAFETY: both libraries wrap the public IOSurfaceRef type. `surface`
            // owns the reference for this borrow; Metal retains it in each texture.
            let surface = unsafe { &*surface.as_ptr().cast::<IOSurfaceRef>() };
            let mut textures = Vec::with_capacity(2);
            for (plane, format) in super::PLANE_FORMATS.iter().enumerate() {
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
                    .ok_or(EncoderError::Invalid(
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
        }))
    }

    pub(super) fn acquire(&mut self) -> Result<Surface, EncoderError> {
        self.free
            .pop()
            .ok_or(EncoderError::Invalid("No free export surface"))
    }

    pub(super) fn encode(
        &mut self,
        surface: Surface,
        index: u64,
    ) -> Result<ExportFrame, EncoderError> {
        let rate = self.settings.frame_rate;
        let value = index
            .checked_mul(u64::from(rate.denominator()))
            .and_then(|value| i64::try_from(value).ok())
            .ok_or(EncoderError::Invalid("Video timestamp is too large"))?;
        let scale = i32::try_from(rate.numerator()).map_err(EncoderError::pipeline)?;
        let image = surface
            .pixel
            .io_surface()
            .ok_or(EncoderError::Invalid("CoreVideo frame has no IOSurface"))?;
        let encoded = self
            .session
            .encode(&image, CMTime::new(value, scale))
            .map_err(EncoderError::pipeline)?;
        let sample = encoded.cm_sample_buffer().ok_or(EncoderError::Invalid(
            "VideoToolbox dropped an export frame",
        ))?;
        let parameters = sample
            .format_description()
            .ok_or(EncoderError::Invalid(
                "H.264 frame has no format description",
            ))?
            .video_parameter_sets()
            .map_err(EncoderError::pipeline)?;
        let length =
            usize::try_from(parameters.nal_unit_header_length).map_err(EncoderError::pipeline)?;
        if !(1..=4).contains(&length) {
            return Err(EncoderError::Invalid("Invalid H.264 NAL length"));
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
                .ok_or(EncoderError::Invalid("Truncated H.264 NAL length"))?;
            let size = prefix
                .iter()
                .fold(0usize, |size, byte| (size << 8) | usize::from(*byte));
            input = &input[length..];
            let nal = input
                .get(..size)
                .ok_or(EncoderError::Invalid("Truncated H.264 NAL unit"))?;
            data.extend_from_slice(&[0, 0, 0, 1]);
            data.extend_from_slice(nal);
            input = &input[size..];
        }
        self.free.push(surface);
        Ok(ExportFrame {
            index,
            data,
            keyframe,
        })
    }
}
