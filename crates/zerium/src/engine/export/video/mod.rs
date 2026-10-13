use std::{
    collections::VecDeque,
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

enum Target {
    Readback,
    #[cfg(any(target_os = "linux", target_os = "windows"))]
    GpuVideo(::gpu_video::EncodeTexture),
    #[cfg(target_os = "macos")]
    VideoToolbox(video_toolbox::Surface),
}

enum PendingData {
    Readback(readback::PendingReadback),
    #[cfg(any(target_os = "linux", target_os = "windows"))]
    GpuVideo,
    #[cfg(target_os = "macos")]
    VideoToolbox(video_toolbox::Surface),
}

struct PendingFrame {
    index: u64,
    data: PendingData,
    submission: wgpu::SubmissionIndex,
}

pub(crate) struct ExportVideoPipeline {
    failed: Arc<AtomicBool>,
    renderer: RenderSession,
    size: RenderSize,
    backend: Backend,
    _source: wgpu::Texture,
    view: wgpu::TextureView,
    cpu_planes: Vec<wgpu::Texture>,
    binding: wgpu::BindGroup,
    pipelines: Vec<wgpu::RenderPipeline>,
    pending: VecDeque<PendingFrame>,
    depth: usize,
}

impl ExportVideoPipeline {
    pub(crate) fn new(
        renderer: RenderSession,
        size: RenderSize,
        depth: usize,
        settings: EncodingSettings,
    ) -> Result<(Self, VideoEncoderInput), EncodingError> {
        crate::engine::gstreamer::initialize().map_err(EncodingError::backend)?;
        settings.output.validate()?;
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
                eprintln!(
                    "GPU video encoder could not be initialized; using CPU readback: {error}"
                );
                None
            }
        };
        let (backend, encoding) = match native {
            Some(backend) => (backend, VideoEncoderInput::H264),
            None => {
                let readback = readback::Nv12Readback::new(device, settings)?;
                let encoding = VideoEncoderInput::Nv12;
                (Backend::Readback(readback), encoding)
            }
        };
        eprintln!(
            "Video export backend: {}",
            match &backend {
                Backend::Readback(_) => "CPU readback / x264",
                #[cfg(any(target_os = "linux", target_os = "windows"))]
                Backend::GpuVideo(_) => "Vulkan Video",
                #[cfg(target_os = "macos")]
                Backend::VideoToolbox(_) => "VideoToolbox",
            }
        );
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
        let pipelines = PLANE_FORMATS
            .iter()
            .enumerate()
            .map(|(plane, format)| {
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
                            format: *format,
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
            })
            .collect();
        let cpu_planes = if matches!(backend, Backend::Readback(_)) {
            PLANE_FORMATS
                .iter()
                .enumerate()
                .map(|(plane, format)| {
                    let size = if plane == 0 {
                        size
                    } else {
                        RenderSize {
                            width: size.width / 2,
                            height: size.height / 2,
                        }
                    };
                    texture(
                        device,
                        size,
                        *format,
                        wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
                    )
                })
                .collect()
        } else {
            Vec::new()
        };
        Ok((
            Self {
                failed: Arc::new(AtomicBool::new(false)),
                renderer,
                size,
                backend,
                _source: source,
                view,
                cpu_planes,
                binding,
                pipelines,
                pending: VecDeque::with_capacity(depth),
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
        let target = match &mut self.backend {
            Backend::Readback(_) => Target::Readback,
            #[cfg(any(target_os = "linux", target_os = "windows"))]
            Backend::GpuVideo(encoder) => Target::GpuVideo(encoder.acquire()?),
            #[cfg(target_os = "macos")]
            Backend::VideoToolbox(encoder) => Target::VideoToolbox(encoder.acquire()?),
        };
        self.renderer
            .submit_scene(scene, &self.view)
            .map_err(EncodingError::backend)?;
        let gpu = self.renderer.gpu();
        let device = &gpu.device;
        let queue = &gpu.queue;
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("zerium-export-color"),
        });
        let views: Vec<_> = match &target {
            Target::Readback => self
                .cpu_planes
                .iter()
                .map(|texture| texture.create_view(&Default::default()))
                .collect(),
            #[cfg(any(target_os = "linux", target_os = "windows"))]
            Target::GpuVideo(target) => target
                .texture()
                .planes()
                .iter()
                .map(|plane| {
                    plane.create_view_with_usage(None, Some(wgpu::TextureUsages::RENDER_ATTACHMENT))
                })
                .collect(),
            #[cfg(target_os = "macos")]
            Target::VideoToolbox(target) => target.views(),
        };
        for (pipeline, view) in self.pipelines.iter().zip(&views) {
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
        let data = match (&mut self.backend, target) {
            (Backend::Readback(readback), Target::Readback) => {
                PendingData::Readback(readback.copy(device, &mut encoder, &self.cpu_planes))
            }
            #[cfg(any(target_os = "linux", target_os = "windows"))]
            (Backend::GpuVideo(native), Target::GpuVideo(target)) => {
                let submission = queue.submit([encoder.finish()]);
                native.submit(index, target)?;
                self.pending.push_back(PendingFrame {
                    index,
                    data: PendingData::GpuVideo,
                    submission,
                });
                return Ok(());
            }
            #[cfg(target_os = "macos")]
            (Backend::VideoToolbox(_), Target::VideoToolbox(surface)) => {
                let retained = surface.pixel.clone();
                let submission = queue.submit([encoder.finish()]);
                queue.on_submitted_work_done(move || drop(retained));
                self.pending.push_back(PendingFrame {
                    index,
                    data: PendingData::VideoToolbox(surface),
                    submission,
                });
                return Ok(());
            }
            _ => return Err(EncodingError::Invalid("Export backend changed")),
        };
        let submission = queue.submit([encoder.finish()]);
        self.pending.push_back(PendingFrame {
            index,
            data,
            submission,
        });
        Ok(())
    }

    pub(crate) fn is_full(&self) -> bool {
        self.pending.len() == self.depth
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
        let Some(pending) = self.pending.pop_front() else {
            return Ok(None);
        };
        #[cfg(any(target_os = "linux", target_os = "windows"))]
        if let Backend::GpuVideo(encoder) = &mut self.backend {
            return encoder
                .receive(
                    pending.index,
                    Cancellation {
                        requested: cancelled,
                        failed: &self.failed,
                    },
                )
                .map(Some);
        }
        let deadline = Instant::now() + WAIT_TIMEOUT;
        loop {
            self.cancellation(cancelled).check()?;
            match self.renderer.gpu().device.poll(wgpu::PollType::Wait {
                submission_index: Some(pending.submission.clone()),
                timeout: Some(Duration::from_millis(10)),
            }) {
                Ok(_) => break,
                Err(wgpu::PollError::Timeout) if Instant::now() < deadline => {}
                Err(error) => return Err(EncodingError::backend(error)),
            }
        }
        let frame = match (&mut self.backend, pending.data) {
            (Backend::Readback(readback), PendingData::Readback(frame)) => VideoEncoderFrame {
                index: pending.index,
                data: readback.finish(frame)?,
                keyframe: false,
            },
            #[cfg(target_os = "macos")]
            (Backend::VideoToolbox(encoder), PendingData::VideoToolbox(surface)) => {
                encoder.encode(surface, pending.index)?
            }
            _ => return Err(EncodingError::Invalid("Export backend changed")),
        };
        Ok(Some(frame))
    }

    pub(crate) fn finish(&mut self) -> Result<(), EncodingError> {
        #[cfg(any(target_os = "linux", target_os = "windows"))]
        if let Backend::GpuVideo(encoder) = &mut self.backend {
            encoder.flush()?;
        }
        Ok(())
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
