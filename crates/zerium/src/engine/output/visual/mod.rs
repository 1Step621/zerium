use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};
use zerium_core::timeline::{
    ExportBitDepth, ExportColorSettings, ExportColorSpace, ExportContainer, VideoCodec,
};

use super::{
    EncodingError, EncodingSettings, VideoEncoderFrame, VideoEncoderInput, VideoPixelFormat,
};
use crate::engine::processing::visual::{RenderScene, RenderSession, RenderSize};

mod backend;
mod color;

use self::{
    backend::Backend,
    color::{ColorConversion, plane_formats},
};
pub(crate) use backend::MediaWriter;

const WAIT_TIMEOUT: Duration = Duration::from_secs(30);

/// Native adapters currently accept 8-bit BT.709. Device capabilities are checked on creation.
fn supports_gpu_encoding(codec: VideoCodec, color: ExportColorSettings) -> bool {
    (codec == VideoCodec::H264 || codec == VideoCodec::H265)
        && color.depth == ExportBitDepth::Eight
        && color.space == ExportColorSpace::Bt709
}

struct PendingFrame<T> {
    index: u64,
    data: T,
    submission: wgpu::SubmissionIndex,
}

pub(crate) struct VideoOutput {
    failed: Arc<AtomicBool>,
    renderer: RenderSession,
    size: RenderSize,
    backend: Backend,
    conversion: ColorConversion,
    depth: usize,
}

impl VideoOutput {
    pub(crate) fn new(
        renderer: RenderSession,
        depth: usize,
        settings: EncodingSettings,
    ) -> Result<(Self, VideoEncoderInput), EncodingError> {
        let dimensions = settings.output.validate()?;
        let format = settings.pixel_format();
        if format != VideoPixelFormat::Rgba
            && (!dimensions.width.is_multiple_of(2) || !dimensions.height.is_multiple_of(2))
        {
            return Err(EncodingError::Invalid(
                "YUV420 output requires even dimensions",
            ));
        }
        let size = RenderSize {
            width: dimensions.width,
            height: dimensions.height,
        };
        let device = &renderer.gpu().device;
        if size.width > device.limits().max_texture_dimension_2d
            || size.height > device.limits().max_texture_dimension_2d
        {
            return Err(EncodingError::Invalid(
                "Export dimensions exceed the renderer's GPU limits",
            ));
        }
        let depth = depth.max(1);
        let use_native = settings.container != ExportContainer::Gif
            && supports_gpu_encoding(settings.video_codec, settings.color);
        #[cfg(any(target_os = "linux", target_os = "windows"))]
        let native = if !use_native {
            Ok(None)
        } else {
            backend::gpu_video::Encoder::new(device, &renderer.gpu().queue, settings, depth)
                .map(|encoder| encoder.map(Backend::GpuVideo))
        };
        #[cfg(target_os = "macos")]
        let native = if !use_native {
            Ok(None)
        } else {
            backend::video_toolbox::Encoder::new(device, settings, depth)
                .map(|encoder| encoder.map(Backend::VideoToolbox))
        };
        let native = match native {
            Ok(native) => native,
            Err(error) => {
                log::warn!(
                    "GPU video encoder could not be initialized; using CPU readback: {error}"
                );
                None
            }
        };
        let (backend, encoding) = match native {
            Some(backend) => (backend, VideoEncoderInput::Encoded),
            None => {
                let readback = backend::Readback::new(device, dimensions, format)?;
                let encoding = VideoEncoderInput::Raw(format);
                (Backend::Readback(readback), encoding)
            }
        };
        log::info!(
            "Video export backend: {}",
            match &backend {
                Backend::Readback(_) => "CPU readback",
                #[cfg(any(target_os = "linux", target_os = "windows"))]
                Backend::GpuVideo(_) => "Vulkan Video",
                #[cfg(target_os = "macos")]
                Backend::VideoToolbox(_) => "VideoToolbox",
            }
        );
        let conversion = ColorConversion::new(device, size, settings.color, format);
        Ok((
            Self {
                failed: Arc::new(AtomicBool::new(false)),
                renderer,
                size,
                backend,
                conversion,
                depth,
            },
            encoding,
        ))
    }

    pub(crate) fn submit(
        &mut self,
        index: u64,
        scene: &RenderScene,
        cancelled: &AtomicBool,
    ) -> Result<(), EncodingError> {
        self.cancellation(cancelled).check()?;
        if self.is_full() {
            return Err(EncodingError::Invalid("Export frame pipeline is full"));
        }
        if scene.size != self.size {
            return Err(EncodingError::Invalid("Export dimensions changed"));
        }
        match &mut self.backend {
            Backend::Readback(readback) => {
                readback.submit(index, &self.renderer, &self.conversion, scene)
            }
            #[cfg(any(target_os = "linux", target_os = "windows"))]
            Backend::GpuVideo(encoder) => {
                encoder.submit(index, &self.renderer, &self.conversion, scene)
            }
            #[cfg(target_os = "macos")]
            Backend::VideoToolbox(encoder) => {
                encoder.submit(index, &self.renderer, &self.conversion, scene)
            }
        }
    }

    pub(crate) fn is_full(&self) -> bool {
        let pending = match &self.backend {
            Backend::Readback(readback) => readback.pending_frames(),
            #[cfg(any(target_os = "linux", target_os = "windows"))]
            Backend::GpuVideo(encoder) => encoder.pending_frames(),
            #[cfg(target_os = "macos")]
            Backend::VideoToolbox(encoder) => encoder.pending_frames(),
        };
        pending >= self.depth
    }

    pub(crate) fn failure_signal(&self) -> Arc<AtomicBool> {
        self.failed.clone()
    }

    fn cancellation<'a>(&'a self, requested: &'a AtomicBool) -> Cancellation<'a> {
        Cancellation {
            requested,
            failed: &self.failed,
        }
    }

    pub(crate) fn finish_next(
        &mut self,
        cancelled: &AtomicBool,
    ) -> Result<Option<VideoEncoderFrame>, EncodingError> {
        let cancelled = Cancellation {
            requested: cancelled,
            failed: &self.failed,
        };
        match &mut self.backend {
            Backend::Readback(readback) => {
                readback.finish_next(&self.renderer.gpu().device, cancelled)
            }
            #[cfg(any(target_os = "linux", target_os = "windows"))]
            Backend::GpuVideo(encoder) => encoder.finish_next(cancelled),
            #[cfg(target_os = "macos")]
            Backend::VideoToolbox(encoder) => {
                encoder.finish_next(&self.renderer.gpu().device, cancelled)
            }
        }
    }

    pub(crate) fn finish(&mut self) -> Result<(), EncodingError> {
        #[cfg(any(target_os = "linux", target_os = "windows"))]
        if let Backend::GpuVideo(encoder) = &mut self.backend {
            encoder.flush()?;
        }
        Ok(())
    }
}

fn wait_for_submission(
    device: &wgpu::Device,
    submission: wgpu::SubmissionIndex,
    cancelled: Cancellation<'_>,
) -> Result<(), EncodingError> {
    let deadline = Instant::now() + WAIT_TIMEOUT;
    loop {
        cancelled.check()?;
        match device.poll(wgpu::PollType::Wait {
            submission_index: Some(submission.clone()),
            timeout: Some(Duration::from_millis(10)),
        }) {
            Ok(_) => return Ok(()),
            Err(wgpu::PollError::Timeout) if Instant::now() < deadline => {}
            Err(error) => return Err(EncodingError::backend(error)),
        }
    }
}

#[derive(Clone, Copy)]
pub(crate) struct Cancellation<'a> {
    requested: &'a AtomicBool,
    failed: &'a AtomicBool,
}

impl Cancellation<'_> {
    fn check(self) -> Result<(), EncodingError> {
        if self.requested.load(Ordering::Relaxed) {
            return Err(EncodingError::Cancelled);
        }
        if self.failed.load(Ordering::Acquire) {
            return Err(EncodingError::Invalid(
                "Video encoder terminated before export finished",
            ));
        }
        Ok(())
    }
}

pub(crate) fn texture(
    device: &wgpu::Device,
    size: RenderSize,
    format: wgpu::TextureFormat,
    usage: wgpu::TextureUsages,
) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some("zerium-export-surface"),
        size: wgpu::Extent3d {
            width: size.width,
            height: size.height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage,
        view_formats: &[],
    })
}
