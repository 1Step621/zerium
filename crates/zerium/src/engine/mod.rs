//! Media engine stages and the native resources shared by their consumers.

pub(crate) mod input;
pub(crate) mod output;
pub(crate) mod processing;

use std::sync::OnceLock;

use gpu_video::VideoAdapterExt;
use gst::prelude::*;
use gstreamer as gst;

#[derive(Clone)]
pub(crate) struct GpuContext {
    pub(crate) device: wgpu::Device,
    pub(crate) queue: wgpu::Queue,
}

impl GpuContext {
    pub(crate) fn new(device: wgpu::Device, queue: wgpu::Queue) -> Self {
        device.set_device_lost_callback(|reason, message| {
            log::error!("GPU device lost ({reason:?}): {message}");
        });
        Self { device, queue }
    }

    pub(crate) fn request(
        adapter: &wgpu::Adapter,
        descriptor: &wgpu::DeviceDescriptor<'_>,
    ) -> Result<Self, wgpu::RequestDeviceError> {
        let mut descriptor = descriptor.clone();
        // Device limits stay at the requested values even if the adapter supports more.
        descriptor.required_limits.max_texture_dimension_2d =
            adapter.limits().max_texture_dimension_2d;

        if adapter
            .video_adapter_info()
            .is_some_and(|info| info.supports_decoding || info.supports_encoding)
        {
            match adapter.request_device_with_video_support(
                &gpu_video::parameters::VideoDeviceDescriptor {
                    wgpu_features: descriptor.required_features,
                    wgpu_experimental_features: descriptor.experimental_features,
                    wgpu_limits: descriptor.required_limits.clone(),
                },
            ) {
                Ok((device, queue)) => return Ok(Self::new(device, queue)),
                Err(error) => log::warn!(
                    "Video GPU device initialization failed; using a regular GPU device: {error}"
                ),
            }
        }
        pollster::block_on(adapter.request_device(&descriptor))
            .map(|(device, queue)| Self::new(device, queue))
    }
}

fn initialize_gstreamer() -> Result<(), String> {
    static INIT: OnceLock<Result<(), String>> = OnceLock::new();
    INIT.get_or_init(|| {
        gstreamer::init().map_err(|error| error.to_string())?;
        log::info!("GStreamer {} initialized", gstreamer::version_string());
        Ok(())
    })
    .clone()
}

/// Configure relocated release bundles before any application threads start.
pub(crate) fn configure_gstreamer_bundle() {
    let Some(directory) = std::env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(ToOwned::to_owned))
    else {
        return;
    };
    #[cfg(target_os = "macos")]
    let (plugins, scanner) = (
        directory.join("../PlugIns/gstreamer"),
        directory.join("gst-plugin-scanner"),
    );
    #[cfg(target_os = "windows")]
    let (plugins, scanner) = (
        directory.join("lib/gstreamer-1.0"),
        directory.join("libexec/gstreamer-1.0/gst-plugin-scanner.exe"),
    );
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let (plugins, scanner) = (
        directory.join("../lib/gstreamer-1.0"),
        directory.join("../libexec/gstreamer-1.0/gst-plugin-scanner"),
    );
    // SAFETY: main calls this before creating threads or initializing libraries.
    unsafe {
        #[cfg(target_os = "windows")]
        if plugins.is_dir() {
            let existing = std::env::var_os("PATH").unwrap_or_default();
            if let Ok(path) = std::env::join_paths(
                std::iter::once(directory).chain(std::env::split_paths(&existing)),
            ) {
                std::env::set_var("PATH", path);
            }
        }
        if plugins.is_dir() && std::env::var_os("GST_PLUGIN_SYSTEM_PATH_1_0").is_none() {
            std::env::set_var("GST_PLUGIN_SYSTEM_PATH_1_0", plugins);
        }
        if scanner.is_file() && std::env::var_os("GST_PLUGIN_SCANNER_1_0").is_none() {
            std::env::set_var("GST_PLUGIN_SCANNER_1_0", scanner);
        }
    }
}

/// Always stop streaming threads, including when setup or seeking fails.
struct GstreamerPipeline(gst::Pipeline);

impl GstreamerPipeline {
    fn new(pipeline: gst::Pipeline) -> Self {
        Self(pipeline)
    }
}

impl std::ops::Deref for GstreamerPipeline {
    type Target = gst::Pipeline;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl Drop for GstreamerPipeline {
    fn drop(&mut self) {
        let _ = self.set_state(gst::State::Null);
    }
}
