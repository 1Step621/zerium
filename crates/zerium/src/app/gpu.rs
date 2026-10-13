//! Adapt GPUI's surface-device creation to the engine's video-capable GPU setup.
use crate::engine::gpu::GpuContext;
use std::sync::Arc;

pub(super) fn wgpu_options() -> gpui::WgpuOptions {
    gpui::WgpuOptions {
        preferred_backends: if cfg!(target_os = "macos") {
            wgpu::Backends::METAL
        } else {
            wgpu::Backends::VULKAN
        },
        device_factory: Some(Arc::new(|adapter, descriptor| {
            GpuContext::request(adapter, descriptor)
                .map(|gpu| (gpu.device, gpu.queue))
                .map_err(Into::into)
        })),
        ..Default::default()
    }
}
