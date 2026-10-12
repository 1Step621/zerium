//! Platform imports stay here; decoding, conversion and synchronization helpers
//! are supplied by GStreamer, wgpu-hal, and the operating system libraries.
use bytemuck::Zeroable;
use gstreamer as gst;
use gstreamer_video as video;
use wgpu::util::DeviceExt;

use crate::engine::rendering::{RenderError, VideoInputPipeline};
use crate::engine::{
    frame::YuvConversion,
    gstreamer::input::{VideoFrame, VideoTransfer, yuv_format},
};

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
use linux::import as import_planes;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
use macos::import as import_planes;
#[cfg(target_os = "windows")]
mod windows;
#[cfg(target_os = "windows")]
use windows::import as import_planes;
#[cfg(any(target_os = "linux", target_os = "windows"))]
mod vulkan;

pub(crate) struct NativeVideoPipeline(VideoInputPipeline);

pub(crate) struct NativeVideoInput {
    buffer: gst::Buffer,
    planes: Vec<wgpu::Texture>,
    conversion: Conversion,
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Conversion {
    yuv: YuvConversion,
    mode: [u32; 4],
}

impl NativeVideoPipeline {
    pub(crate) fn new(device: &wgpu::Device) -> Result<Self, RenderError> {
        Ok(Self(VideoInputPipeline::new(
            device,
            "zerium.native.video",
            include_str!("native_input.wgsl"),
        )?))
    }

    /// Only the first import chooses a route. A later GPU import failure is an
    /// error; CPU selection is shared with the decoder for the whole session.
    pub(crate) fn import(
        device: &wgpu::Device,
        frame: &VideoFrame,
    ) -> Result<Option<NativeVideoInput>, RenderError> {
        let buffer = frame
            .buffer()
            .ok_or_else(|| RenderError::backend("Video frame is already GPU resident"))?;
        let conversion = conversion(&frame.info);
        let mut first_import = None;
        let transfer = frame.transfer.get_or_init(|| {
            first_import = conversion.and_then(|_| import_planes(device, &buffer, frame));
            if first_import.is_some() {
                VideoTransfer::Gpu(device.clone())
            } else {
                VideoTransfer::Cpu
            }
        });
        let VideoTransfer::Gpu(owner) = transfer else {
            return Ok(None);
        };
        if owner != device {
            return Err(RenderError::backend(
                "A GPU video session cannot use a different device",
            ));
        }
        let conversion = conversion
            .ok_or_else(|| RenderError::backend("Video format changed during GPU transfer"))?;
        let planes = first_import
            .or_else(|| import_planes(device, &buffer, frame))
            .ok_or_else(|| {
                RenderError::backend("GPU video import failed during an active session")
            })?;
        Ok(Some(NativeVideoInput {
            buffer,
            planes,
            conversion,
        }))
    }

    pub(crate) fn convert(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        input: NativeVideoInput,
        target: &wgpu::Texture,
    ) {
        let NativeVideoInput {
            buffer,
            planes,
            conversion,
        } = input;
        let views: Vec<_> = planes
            .iter()
            .map(|plane| plane.create_view(&Default::default()))
            .collect();
        let uniform = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("zerium-native-video-color"),
            contents: bytemuck::bytes_of(&conversion),
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let binding = self.0.binding(
            device,
            [&views[0], views.get(1).unwrap_or(&views[0])],
            &uniform,
        );
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("zerium-native-video-copy"),
        });
        #[cfg(any(target_os = "linux", target_os = "windows"))]
        let acquire = {
            let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("zerium-native-video-acquire"),
            });
            vulkan::transfer_ownership(device, &mut encoder, &planes, true);
            encoder.finish()
        };
        self.0.draw(&mut encoder, &binding, target);
        #[cfg(any(target_os = "linux", target_os = "windows"))]
        let release = {
            let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("zerium-native-video-release"),
            });
            vulkan::transfer_ownership(device, &mut encoder, &planes, false);
            encoder.finish()
        };
        // Only DX12 imports return to COMMON/PRESENT; Vulkan releases to GENERAL.
        // SAFETY: read-only backend detection does not access raw GPU resources.
        #[cfg(target_os = "windows")]
        if unsafe { device.as_hal::<wgpu::hal::api::Dx12>() }.is_some() {
            encoder.transition_resources(
                std::iter::empty(),
                planes.iter().map(|texture| wgpu::TextureTransition {
                    texture,
                    selector: None,
                    state: wgpu::TextureUses::PRESENT,
                }),
            );
        }
        #[cfg(any(target_os = "linux", target_os = "windows"))]
        queue.submit([acquire, encoder.finish(), release]);
        #[cfg(not(any(target_os = "linux", target_os = "windows")))]
        queue.submit([encoder.finish()]);
        // The producer cannot reuse its surface until the GPU read finishes.
        queue.on_submitted_work_done(move || drop((buffer, planes)));
    }
}

#[cfg(not(any(target_os = "linux", target_os = "windows", target_os = "macos")))]
fn import_planes(
    _device: &wgpu::Device,
    _buffer: &gst::BufferRef,
    _info: &video::VideoInfo,
) -> Option<Vec<wgpu::Texture>> {
    None
}

fn conversion(info: &video::VideoInfo) -> Option<Conversion> {
    let (yuv, mode) = match info.format() {
        video::VideoFormat::Nv12 => (yuv_format(info, [2, 2], 8)?.conversion(), 1),
        video::VideoFormat::Rgba | video::VideoFormat::Bgra => (YuvConversion::zeroed(), 0),
        _ => return None,
    };
    Some(Conversion {
        yuv,
        mode: [mode, 0, 0, 0],
    })
}

fn descriptor(
    width: u32,
    height: u32,
    format: wgpu::TextureFormat,
) -> wgpu::TextureDescriptor<'static> {
    wgpu::TextureDescriptor {
        label: Some("zerium-imported-video-plane"),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    }
}

/// GPUI and the export renderer enable supported import features at creation.
pub(crate) fn optional_features() -> wgpu::Features {
    #[cfg(target_os = "linux")]
    {
        wgpu::Features::VULKAN_EXTERNAL_MEMORY_DMA_BUF
    }
    #[cfg(target_os = "windows")]
    {
        wgpu::Features::VULKAN_EXTERNAL_MEMORY_WIN32
    }
    #[cfg(not(any(target_os = "linux", target_os = "windows")))]
    {
        wgpu::Features::empty()
    }
}
