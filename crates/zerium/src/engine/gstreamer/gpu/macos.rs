use crate::engine::gstreamer::input::VideoFrame;
use gstreamer as gst;
use gstreamer_video as video;
use objc2_io_surface::IOSurfaceRef;
use objc2_metal::{
    MTLDevice, MTLPixelFormat, MTLTextureDescriptor, MTLTextureType, MTLTextureUsage,
};
use std::{ptr, sync::OnceLock};

// This public GStreamer 1.30 API avoids depending on applemedia's private memory
// layout. Older runtimes remain supported through the CPU YUV fallback.
type PeekSurface =
    unsafe extern "C" fn(*mut gst::ffi::GstMemory, *mut *const IOSurfaceRef, *mut u32) -> i32;
struct SurfaceApi {
    _library: libloading::Library,
    peek: PeekSurface,
}

fn surface_api() -> Option<&'static SurfaceApi> {
    static API: OnceLock<Option<SurfaceApi>> = OnceLock::new();
    API.get_or_init(|| unsafe {
        let bundled = std::env::current_exe()
            .ok()?
            .parent()?
            .join("../Frameworks/libgstallocators-1.0.0.dylib");
        let library = libloading::Library::new(bundled)
            .or_else(|_| libloading::Library::new("libgstallocators-1.0.0.dylib"))
            .ok()?;
        let peek = *library
            .get::<PeekSurface>(b"gst_iosurface_memory_peek_surface\0")
            .ok()?;
        Some(SurfaceApi {
            _library: library,
            peek,
        })
    })
    .as_ref()
}

pub(super) fn import(
    device: &wgpu::Device,
    buffer: &gst::BufferRef,
    frame: &VideoFrame,
) -> Option<Vec<wgpu::Texture>> {
    let info = &frame.info;
    if info.format() != video::VideoFormat::Nv12 || buffer.n_memory() < 2 {
        return None;
    }
    let api = surface_api()?;
    let mut planes = Vec::with_capacity(2);
    // SAFETY: peek returns IOSurfaces borrowed from the retained GstBuffer. Metal
    // textures retain the surface, and the source buffer is retained until the
    // conversion completes. All textures are created on wgpu's own MTLDevice.
    unsafe {
        let hal = device.as_hal::<wgpu::hal::api::Metal>()?;
        for plane in 0..2 {
            let memory = buffer.peek_memory(plane);
            let mut surface: *const IOSurfaceRef = ptr::null();
            let mut index = 0;
            if (api.peek)(memory.as_mut_ptr(), &mut surface, &mut index) == 0 {
                return None;
            }
            let surface = surface.as_ref()?;
            let (width, height, format, metal_format) = if plane == 0 {
                (
                    info.width(),
                    info.height(),
                    wgpu::TextureFormat::R8Unorm,
                    MTLPixelFormat::R8Unorm,
                )
            } else {
                (
                    info.width().div_ceil(2),
                    info.height().div_ceil(2),
                    wgpu::TextureFormat::Rg8Unorm,
                    MTLPixelFormat::RG8Unorm,
                )
            };
            if index as usize >= surface.plane_count()
                || surface.width_of_plane(index as usize) != width as usize
                || surface.height_of_plane(index as usize) != height as usize
            {
                return None;
            }
            let metal_desc = MTLTextureDescriptor::new();
            metal_desc.setTextureType(MTLTextureType::Type2D);
            metal_desc.setPixelFormat(metal_format);
            metal_desc.setWidth(width as usize);
            metal_desc.setHeight(height as usize);
            metal_desc.setUsage(MTLTextureUsage::ShaderRead);
            let raw = hal.raw_device().newTextureWithDescriptor_iosurface_plane(
                &metal_desc,
                surface,
                index as usize,
            )?;
            let desc = super::descriptor(width, height, format);
            let texture = wgpu::hal::metal::Device::texture_from_raw(
                raw,
                format,
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
            planes.push(device.create_texture_from_hal::<wgpu::hal::api::Metal>(
                texture,
                &desc,
                wgpu::TextureUses::RESOURCE,
            ));
        }
    }
    Some(planes)
}
