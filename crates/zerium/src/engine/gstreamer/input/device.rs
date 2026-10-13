//! Per-pipeline selection of decoder and converter elements on the renderer GPU.
use gst::prelude::*;
use gstreamer as gst;

#[derive(Clone)]
pub(super) struct DecoderDevice {
    #[cfg(target_os = "linux")]
    render_node: std::path::PathBuf,
    #[cfg(target_os = "windows")]
    adapter_luid: i64,
    #[cfg(target_os = "windows")]
    context: gst::Context,
}

impl DecoderDevice {
    pub(super) fn for_device(device: &wgpu::Device) -> Option<Self> {
        #[cfg(target_os = "linux")]
        {
            linux::for_device(device)
        }
        #[cfg(target_os = "windows")]
        {
            windows::for_device(device)
        }
        #[cfg(target_os = "macos")]
        {
            // VideoToolbox does not expose a physical GPU selector.
            let _ = device;
            Some(Self {})
        }
        #[cfg(not(any(target_os = "linux", target_os = "windows", target_os = "macos")))]
        {
            let _ = device;
            None
        }
    }

    pub(super) fn configure_pipeline(&self, pipeline: &gst::Pipeline) {
        #[cfg(target_os = "windows")]
        pipeline.set_context(&self.context);
        // GstBin propagates contexts to its children, including dynamic decoders.
        #[cfg(not(target_os = "windows"))]
        let _ = pipeline;
    }

    /// Bind the native-memory backend and physical GPU before starting decode.
    fn matches_factory(&self, factory: &gst::ElementFactory) -> bool {
        #[cfg(target_os = "linux")]
        let memory = "memory:VAMemory";
        #[cfg(target_os = "windows")]
        let memory = "memory:D3D12Memory";
        #[cfg(any(target_os = "linux", target_os = "windows"))]
        {
            let caps = gst::Caps::builder("video/x-raw").features([memory]).build();
            factory.static_pad_templates().iter().any(|pad| {
                pad.direction() == gst::PadDirection::Src && pad.caps().can_intersect(&caps)
            }) && factory
                .create()
                .build()
                .is_ok_and(|element| self.matches_element(&element))
        }
        #[cfg(not(any(target_os = "linux", target_os = "windows")))]
        {
            let _ = factory;
            true
        }
    }

    #[cfg(any(target_os = "linux", target_os = "windows"))]
    fn matches_element(&self, element: &gst::Element) -> bool {
        // Read the factory's device in NULL state, before it starts decoding.
        // A foreign-GPU factory can reject a shared context and open its own GPU.
        #[cfg(target_os = "linux")]
        {
            element.find_property("device-path").is_some()
                && element
                    .property::<Option<String>>("device-path")
                    .is_some_and(|path| std::path::Path::new(&path) == self.render_node)
        }
        #[cfg(target_os = "windows")]
        {
            element.find_property("adapter-luid").is_some()
                && element.property::<i64>("adapter-luid") == self.adapter_luid
        }
    }

    #[cfg(target_os = "linux")]
    pub(super) fn postprocessor(&self) -> Result<gst::Element, crate::engine::media::MediaError> {
        gst::Registry::get()
            .features(gst::ElementFactory::static_type())
            .into_iter()
            .filter_map(|feature| feature.downcast::<gst::ElementFactory>().ok())
            .filter(|factory| {
                factory.plugin_name().as_deref() == Some("va")
                    && factory.metadata("klass").is_some_and(|klass| {
                        klass.contains("Converter") && klass.contains("Scaler")
                    })
            })
            .filter_map(|factory| factory.create().build().ok())
            .find(|element| self.matches_element(element))
            .ok_or_else(|| {
                crate::engine::media::MediaError::external(
                    "No VA postprocessor on the renderer GPU",
                )
            })
    }
}

/// Keep GStreamer's ranks within each group; skip hardware on foreign GPUs.
pub(super) fn configure_source(source: &gst::Element, device: Option<&DecoderDevice>) {
    if device.is_some() && cfg!(target_os = "macos") {
        return;
    }
    let device = device.cloned();
    source.connect("autoplug-sort", false, move |values| {
        let factories = values[3]
            .get::<gst::glib::ValueArray>()
            .expect("uridecodebin supplies its decoder factory array");
        let mut candidates: Vec<_> = factories
            .iter()
            .filter_map(|value| {
                let factory = value
                    .get::<gst::ElementFactory>()
                    .expect("autoplug-sort contains element factories");
                let klass = factory.metadata("klass").unwrap_or("");
                let priority = if !klass.contains("Decoder/Video") {
                    0
                } else if !klass.contains("Hardware") {
                    1
                } else if device
                    .as_ref()
                    .is_some_and(|device| device.matches_factory(&factory))
                {
                    0
                } else {
                    return None;
                };
                Some((priority, value))
            })
            .collect();
        candidates.sort_by_key(|(priority, _)| *priority);
        Some(gst::glib::ValueArray::new(candidates.into_iter().map(|(_, value)| value)).to_value())
    });
}

#[cfg(target_os = "linux")]
mod linux {
    use super::*;
    use ash::vk;
    use std::os::unix::fs::{FileTypeExt, MetadataExt};

    pub(super) fn for_device(device: &wgpu::Device) -> Option<DecoderDevice> {
        // SAFETY: the HAL guard keeps this device and its physical GPU alive.
        let hal = unsafe { device.as_hal::<wgpu::hal::api::Vulkan>() }?;
        let instance = hal.shared_instance().raw_instance();
        let mut drm = vk::PhysicalDeviceDrmPropertiesEXT::default();
        let mut properties = vk::PhysicalDeviceProperties2::default().push_next(&mut drm);
        unsafe {
            instance.get_physical_device_properties2(hal.raw_physical_device(), &mut properties)
        };
        if drm.has_render != vk::TRUE {
            return None;
        }
        let render_node = std::fs::read_dir("/dev/dri")
            .ok()?
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .find(|path| {
                std::fs::metadata(path).is_ok_and(|metadata| {
                    let device = metadata.rdev() as libc::dev_t;
                    metadata.file_type().is_char_device()
                        && i64::from(libc::major(device)) == drm.render_major
                        && i64::from(libc::minor(device)) == drm.render_minor
                })
            })?;
        eprintln!("Video decoder GPU: VA-API on {}", render_node.display());
        Some(DecoderDevice { render_node })
    }
}

#[cfg(target_os = "windows")]
mod windows {
    use super::*;
    use gstreamer_d3d12::{D3D12Device, ffi, glib::translate::ToGlibPtr};

    pub(super) fn for_device(device: &wgpu::Device) -> Option<DecoderDevice> {
        // SAFETY: each HAL guard keeps the renderer's native device alive.
        let adapter_luid = if let Some(hal) = unsafe { device.as_hal::<wgpu::hal::api::Dx12>() } {
            let luid = unsafe { hal.raw_device().GetAdapterLuid() };
            unsafe { ffi::gst_d3d12_luid_to_int64((&luid as *const _).cast()) }
        } else if let Some(hal) = unsafe { device.as_hal::<wgpu::hal::api::Vulkan>() } {
            let instance = hal.shared_instance().raw_instance();
            let mut id = ash::vk::PhysicalDeviceIDProperties::default();
            let mut properties = ash::vk::PhysicalDeviceProperties2::default().push_next(&mut id);
            unsafe {
                instance.get_physical_device_properties2(hal.raw_physical_device(), &mut properties)
            };
            if id.device_luid_valid != ash::vk::TRUE {
                return None;
            }
            i64::from_le_bytes(id.device_luid)
        } else {
            return None;
        };

        let native_device = D3D12Device::for_adapter_luid(adapter_luid)?;
        // SAFETY: the returned context owns a reference to the D3D12 device.
        let context = unsafe {
            let pointer = ffi::gst_d3d12_context_new(native_device.to_glib_none().0);
            if pointer.is_null() {
                return None;
            }
            gst::glib::translate::from_glib_full(pointer)
        };
        eprintln!("Video decoder GPU: D3D12 adapter LUID {adapter_luid:#x}");
        Some(DecoderDevice {
            adapter_luid,
            context,
        })
    }
}
