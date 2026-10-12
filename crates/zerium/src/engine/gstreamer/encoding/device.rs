use std::sync::Arc;

#[cfg(any(target_os = "linux", target_os = "windows"))]
use gpu_video::VideoAdapterExt;

pub(crate) fn wgpu_options() -> gpui::WgpuOptions {
    gpui::WgpuOptions {
        preferred_backends: if cfg!(target_os = "macos") {
            wgpu::Backends::METAL
        } else {
            wgpu::Backends::VULKAN
        },
        device_factory: Some(Arc::new(|adapter, descriptor| {
            request_device(adapter, descriptor).map_err(Into::into)
        })),
        ..Default::default()
    }
}

pub(crate) fn request_device(
    adapter: &wgpu::Adapter,
    descriptor: &wgpu::DeviceDescriptor<'_>,
) -> Result<(wgpu::Device, wgpu::Queue), wgpu::RequestDeviceError> {
    let mut descriptor = descriptor.clone();
    descriptor.required_features |=
        adapter.features() & crate::engine::gstreamer::gpu::optional_features();
    #[cfg(any(target_os = "linux", target_os = "windows"))]
    if adapter
        .video_adapter_info()
        .is_some_and(|info| info.supports_encoding)
    {
        match adapter.request_device_with_video_support(
            &gpu_video::parameters::VideoDeviceDescriptor {
                wgpu_features: descriptor.required_features,
                wgpu_experimental_features: descriptor.experimental_features,
                wgpu_limits: descriptor.required_limits.clone(),
            },
        ) {
            Ok(device) => return Ok(device),
            Err(error) => eprintln!(
                "Video GPU device initialization failed; using a regular GPU device: {error}"
            ),
        }
    }
    pollster::block_on(adapter.request_device(&descriptor))
}
