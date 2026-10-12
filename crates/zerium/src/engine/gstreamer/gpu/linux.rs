use crate::engine::gstreamer::input::VideoFrame;
use ash::vk;
use gstreamer as gst;
use gstreamer_allocators::DmaBufMemory;
use gstreamer_video as video;
use std::os::fd::{AsRawFd, BorrowedFd, FromRawFd, OwnedFd};

pub(super) fn import(
    device: &wgpu::Device,
    buffer: &gst::BufferRef,
    frame: &VideoFrame,
) -> Option<Vec<wgpu::Texture>> {
    let info = &frame.info;
    let drm_modifier = frame.drm_modifier?;
    if info.format() != video::VideoFormat::Bgra
        || !device
            .features()
            .contains(wgpu::Features::VULKAN_EXTERNAL_MEMORY_DMA_BUF)
    {
        return None;
    }
    let hal = unsafe { device.as_hal::<wgpu::hal::api::Vulkan>() }?;
    if !supports_modifier(&hal, drm_modifier) {
        eprintln!(
            "DMA-BUF modifier {drm_modifier:#x} is unsupported on {}",
            device.adapter_info().name
        );
        return None;
    }
    if buffer.n_memory() != 1 {
        return None;
    }
    let memory = buffer
        .peek_memory(0)
        .downcast_memory_ref::<DmaBufMemory>()?;
    let meta = buffer.meta::<video::VideoMeta>()?;
    if meta.n_planes() != 1 || meta.width() != info.width() || meta.height() != info.height() {
        return None;
    }
    let stride = u64::try_from(meta.stride()[0]).ok()?;
    if stride < u64::from(info.width()) * 4 {
        return None;
    }
    let offset = u64::try_from(memory.offset().checked_add(meta.offset()[0])?).ok()?;
    // DMA-BUF explicit fence export waits only for the producer, without mapping
    // pixels or submitting unmanaged work onto wgpu's Vulkan queue.
    if !wait_for_producer(memory.fd()) {
        return None;
    }
    let fd = unsafe { BorrowedFd::borrow_raw(memory.fd()) }
        .try_clone_to_owned()
        .ok()?;
    let desc = super::descriptor(info.width(), info.height(), wgpu::TextureFormat::Bgra8Unorm);
    let hal_desc = wgpu::hal::TextureDescriptor {
        label: desc.label,
        size: desc.size,
        mip_level_count: 1,
        sample_count: 1,
        dimension: desc.dimension,
        format: desc.format,
        usage: wgpu::TextureUses::RESOURCE,
        memory_flags: wgpu::hal::MemoryFlags::empty(),
        view_formats: Vec::new(),
    };
    // SAFETY: the negotiated modifier and VideoMeta describe this single-plane
    // BGRA allocation. Ownership barriers bracket our only GPU read.
    let texture = unsafe {
        let texture = hal
            .texture_from_dmabuf_fd(fd, &hal_desc, drm_modifier, stride, offset)
            .ok()?;
        device.create_texture_from_hal::<wgpu::hal::api::Vulkan>(
            texture,
            &desc,
            wgpu::TextureUses::RESOURCE,
        )
    };
    Some(vec![texture])
}

fn supports_modifier(device: &wgpu::hal::vulkan::Device, modifier: u64) -> bool {
    let instance = device.shared_instance().raw_instance();
    let mut list = vk::DrmFormatModifierPropertiesListEXT::default();
    let mut properties = vk::FormatProperties2::default().push_next(&mut list);
    // Query the layouts advertised by this driver before importing. Some drivers
    // accept another vendor's modifier at image creation but render corrupt data.
    unsafe {
        instance.get_physical_device_format_properties2(
            device.raw_physical_device(),
            vk::Format::B8G8R8A8_UNORM,
            &mut properties,
        )
    };
    let mut modifiers = vec![
        vk::DrmFormatModifierPropertiesEXT::default();
        list.drm_format_modifier_count as usize
    ];
    let mut list = vk::DrmFormatModifierPropertiesListEXT::default()
        .drm_format_modifier_properties(&mut modifiers);
    let mut properties = vk::FormatProperties2::default().push_next(&mut list);
    unsafe {
        instance.get_physical_device_format_properties2(
            device.raw_physical_device(),
            vk::Format::B8G8R8A8_UNORM,
            &mut properties,
        )
    };
    modifiers.iter().any(|layout| {
        layout.drm_format_modifier == modifier
            && layout.drm_format_modifier_plane_count == 1
            && layout
                .drm_format_modifier_tiling_features
                .contains(vk::FormatFeatureFlags::SAMPLED_IMAGE)
    })
}

fn wait_for_producer(fd: i32) -> bool {
    #[repr(C)]
    struct ExportSyncFile {
        flags: u32,
        fd: i32,
    }
    // linux/uapi/linux/dma-buf.h: DMA_BUF_IOCTL_EXPORT_SYNC_FILE, DMA_BUF_SYNC_READ.
    let mut request = ExportSyncFile { flags: 1, fd: -1 };
    if unsafe { libc::ioctl(fd, 0xc008_6202_u64 as libc::c_ulong, &mut request) } < 0 {
        return false;
    }
    // SAFETY: a successful EXPORT_SYNC_FILE returns an owned fd.
    let fence = unsafe { OwnedFd::from_raw_fd(request.fd) };
    let mut poll = libc::pollfd {
        fd: fence.as_raw_fd(),
        events: libc::POLLIN,
        revents: 0,
    };
    loop {
        let result = unsafe { libc::poll(&mut poll, 1, 30_000) };
        if result >= 0 {
            return result > 0 && poll.revents & libc::POLLIN != 0;
        }
        if std::io::Error::last_os_error().kind() != std::io::ErrorKind::Interrupted {
            return false;
        }
    }
}
