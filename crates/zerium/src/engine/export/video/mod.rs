use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

use super::encoding::{EncodingError, EncodingSettings, VideoEncoderFrame, VideoEncoderInput};
use crate::engine::rendering::{OUTPUT_FORMAT, RenderScene, RenderSession, RenderSize};

#[cfg(any(target_os = "linux", target_os = "windows"))]
mod gpu_video;
mod readback;
#[cfg(target_os = "macos")]
mod video_toolbox;

const WAIT_TIMEOUT: Duration = Duration::from_secs(30);
const PLANE_FORMATS: [wgpu::TextureFormat; 2] =
    [wgpu::TextureFormat::R8Unorm, wgpu::TextureFormat::Rg8Unorm];

enum Backend {
    Readback(readback::Nv12Readback),
    #[cfg(any(target_os = "linux", target_os = "windows"))]
    GpuVideo(gpu_video::Encoder),
    #[cfg(target_os = "macos")]
    VideoToolbox(video_toolbox::Encoder),
}

struct PendingFrame<T> {
    index: u64,
    data: T,
    submission: wgpu::SubmissionIndex,
}

pub(crate) struct ExportVideoPipeline {
    failed: Arc<AtomicBool>,
    renderer: RenderSession,
    size: RenderSize,
    backend: Backend,
    conversion: ColorConversion,
    depth: usize,
}

/// Shared rendering into the two NV12 planes supplied by a backend.
struct ColorConversion {
    _source: wgpu::Texture,
    view: wgpu::TextureView,
    binding: wgpu::BindGroup,
    pipelines: [wgpu::RenderPipeline; 2],
}

impl ExportVideoPipeline {
    pub(crate) fn new(
        renderer: RenderSession,
        depth: usize,
        settings: EncodingSettings,
    ) -> Result<(Self, VideoEncoderInput), EncodingError> {
        let dimensions = settings.output.validate()?;
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
        #[cfg(any(target_os = "linux", target_os = "windows"))]
        let native = gpu_video::Encoder::new(device, &renderer.gpu().queue, settings, depth)
            .map(|encoder| encoder.map(Backend::GpuVideo));
        #[cfg(target_os = "macos")]
        let native = video_toolbox::Encoder::new(device, settings, depth)
            .map(|encoder| encoder.map(Backend::VideoToolbox));
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
            Some(backend) => (backend, VideoEncoderInput::H264),
            None => {
                let readback = readback::Nv12Readback::new(device, dimensions)?;
                let encoding = VideoEncoderInput::Nv12;
                (Backend::Readback(readback), encoding)
            }
        };
        log::info!(
            "Video export backend: {}",
            match &backend {
                Backend::Readback(_) => "CPU readback / x264",
                #[cfg(any(target_os = "linux", target_os = "windows"))]
                Backend::GpuVideo(_) => "Vulkan Video",
                #[cfg(target_os = "macos")]
                Backend::VideoToolbox(_) => "VideoToolbox",
            }
        );
        let conversion = ColorConversion::new(device, size);
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

impl ColorConversion {
    fn new(device: &wgpu::Device, size: RenderSize) -> Self {
        let source = texture(
            device,
            size,
            OUTPUT_FORMAT,
            wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
        );
        let view = source.create_view(&Default::default());
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("zerium-export-input"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: false },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            }],
        });
        let binding = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("zerium-export-input"),
            layout: &layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&view),
            }],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("zerium-export-color"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("zerium-export-color"),
            source: wgpu::ShaderSource::Wgsl(include_str!("color.wgsl").into()),
        });
        let pipelines = std::array::from_fn(|plane| {
            let format = PLANE_FORMATS[plane];
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("zerium-export-color"),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("vertex"),
                    compilation_options: Default::default(),
                    buffers: &[],
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some(if plane == 0 { "luma" } else { "chroma" }),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format,
                        blend: None,
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                primitive: Default::default(),
                depth_stencil: None,
                multisample: Default::default(),
                multiview_mask: None,
                cache: None,
            })
        });
        Self {
            _source: source,
            view,
            binding,
            pipelines,
        }
    }

    fn render(
        &self,
        renderer: &RenderSession,
        scene: &RenderScene,
        views: &[wgpu::TextureView],
    ) -> Result<wgpu::CommandEncoder, EncodingError> {
        renderer
            .submit_scene(scene, &self.view)
            .map_err(EncodingError::backend)?;
        let mut encoder =
            renderer
                .gpu()
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("zerium-export-color"),
                });
        for (pipeline, view) in self.pipelines.iter().zip(views) {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("zerium-export-color"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                    depth_slice: None,
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(pipeline);
            pass.set_bind_group(0, &self.binding, &[]);
            pass.draw(0..3, 0..1);
        }
        Ok(encoder)
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
pub(super) struct Cancellation<'a> {
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

pub(super) fn texture(
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
