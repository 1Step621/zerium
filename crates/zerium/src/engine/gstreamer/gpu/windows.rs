use crate::engine::gstreamer::input::VideoFrame;
use ash::vk;
use gstreamer as gst;
use gstreamer_d3d12::{D3D12Memory, prelude::*};
use gstreamer_video as video;
use windows::Win32::{
    Foundation::HANDLE,
    Graphics::{
        Direct3D12::{
            D3D12_RESOURCE_DIMENSION_TEXTURE2D, D3D12_RESOURCE_FLAG_ALLOW_SIMULTANEOUS_ACCESS,
            D3D12_RESOURCE_FLAG_DENY_SHADER_RESOURCE, D3D12_TEXTURE_LAYOUT_UNKNOWN, ID3D12Resource,
        },
        Dxgi::Common::DXGI_FORMAT_R8G8B8A8_UNORM,
    },
};

pub(super) fn import(
    device: &wgpu::Device,
    buffer: &gst::BufferRef,
    frame: &VideoFrame,
) -> Option<Vec<wgpu::Texture>> {
    let info = &frame.info;
    if info.format() != video::VideoFormat::Rgba || buffer.n_memory() != 1 {
        return None;
    }
    let memory = buffer.peek_memory(0).downcast_memory_ref::<D3D12Memory>()?;
    if memory.plane_count() != 1 || memory.subresource_index(0)? != 0 {
        return None;
    }
    let resource = memory.resource_handle();
    // SAFETY: GStreamer retains this resource for the lifetime of the buffer.
    let actual = unsafe { resource.GetDesc() };
    if actual.Dimension != D3D12_RESOURCE_DIMENSION_TEXTURE2D
        || actual.Width != u64::from(info.width())
        || actual.Height != info.height()
        || actual.DepthOrArraySize != 1
        || actual.MipLevels != 1
        || actual.Format != DXGI_FORMAT_R8G8B8A8_UNORM
        || actual.Layout != D3D12_TEXTURE_LAYOUT_UNKNOWN
        || actual.SampleDesc.Count != 1
        || !actual
            .Flags
            .contains(D3D12_RESOURCE_FLAG_ALLOW_SIMULTANEOUS_ACCESS)
        || actual
            .Flags
            .contains(D3D12_RESOURCE_FLAG_DENY_SHADER_RESOURCE)
    {
        return None;
    }
    // GStreamer waits for the producer fence without downloading pixel data.
    memory.sync().ok()?;
    let handle = memory.nt_handle()?;
    let desc = super::descriptor(info.width(), info.height(), wgpu::TextureFormat::Rgba8Unorm);
    // SAFETY: descriptors match the validated resource, and the caller retains
    // its GstBuffer until the consumer GPU submission has finished.
    let texture = unsafe {
        if let Some(hal) = device.as_hal::<wgpu::hal::api::Vulkan>() {
            let texture = import_vulkan(&hal, handle, memory.device().adapter_luid(), &desc)?;
            device.create_texture_from_hal::<wgpu::hal::api::Vulkan>(
                texture,
                &desc,
                wgpu::TextureUses::RESOURCE,
            )
        } else {
            let hal = device.as_hal::<wgpu::hal::api::Dx12>()?;
            let mut resource: Option<ID3D12Resource> = None;
            hal.raw_device()
                .OpenSharedHandle(handle, &mut resource)
                .ok()?;
            let texture = wgpu::hal::dx12::Device::texture_from_raw(
                resource?,
                desc.format,
                desc.dimension,
                desc.size,
                1,
                1,
            );
            device.create_texture_from_hal::<wgpu::hal::api::Dx12>(
                texture,
                &desc,
                wgpu::TextureUses::PRESENT,
            )
        }
    };
    Some(vec![texture])
}

fn import_vulkan(
    hal: &wgpu::hal::vulkan::Device,
    handle: HANDLE,
    adapter_luid: i64,
    desc: &wgpu::TextureDescriptor<'_>,
) -> Option<wgpu::hal::vulkan::Texture> {
    if !hal
        .enabled_device_extensions()
        .contains(&ash::khr::external_memory_win32::NAME)
    {
        return None;
    }
    // SAFETY: the NT handle is borrowed from a retained GStreamer D3D12 buffer.
    // Verify the physical adapter and import capability before allocating. The
    // imported image/memory are handed to wgpu-hal, which owns their destruction.
    unsafe {
        let instance = hal.shared_instance().raw_instance();
        let physical = hal.raw_physical_device();
        let device = hal.raw_device();
        let mut id = vk::PhysicalDeviceIDProperties::default();
        let mut properties = vk::PhysicalDeviceProperties2::default().push_next(&mut id);
        instance.get_physical_device_properties2(physical, &mut properties);
        if id.device_luid_valid != vk::TRUE
            || i64::from_le_bytes(id.device_luid) != adapter_luid
            || id.device_node_mask != 1
        {
            return None;
        }
        let handle_type = vk::ExternalMemoryHandleTypeFlags::D3D12_RESOURCE;
        let mut external_info =
            vk::PhysicalDeviceExternalImageFormatInfo::default().handle_type(handle_type);
        let image_format = vk::PhysicalDeviceImageFormatInfo2::default()
            .format(vk::Format::R8G8B8A8_UNORM)
            .ty(vk::ImageType::TYPE_2D)
            .tiling(vk::ImageTiling::OPTIMAL)
            .usage(vk::ImageUsageFlags::SAMPLED)
            .push_next(&mut external_info);
        let mut external_properties = vk::ExternalImageFormatProperties::default();
        let mut format_properties =
            vk::ImageFormatProperties2::default().push_next(&mut external_properties);
        instance
            .get_physical_device_image_format_properties2(
                physical,
                &image_format,
                &mut format_properties,
            )
            .ok()?;
        let limits = format_properties.image_format_properties;
        if !external_properties
            .external_memory_properties
            .external_memory_features
            .contains(vk::ExternalMemoryFeatureFlags::IMPORTABLE)
            || desc.size.width > limits.max_extent.width
            || desc.size.height > limits.max_extent.height
        {
            return None;
        }
        let external = ash::khr::external_memory_win32::Device::new(instance, device);
        let mut handle_properties = vk::MemoryWin32HandlePropertiesKHR::default();
        external
            .get_memory_win32_handle_properties(handle_type, handle.0 as _, &mut handle_properties)
            .ok()?;
        let mut external_image =
            vk::ExternalMemoryImageCreateInfo::default().handle_types(handle_type);
        let image_info = vk::ImageCreateInfo::default()
            .image_type(vk::ImageType::TYPE_2D)
            .format(vk::Format::R8G8B8A8_UNORM)
            .extent(vk::Extent3D {
                width: desc.size.width,
                height: desc.size.height,
                depth: 1,
            })
            .mip_levels(1)
            .array_layers(1)
            .samples(vk::SampleCountFlags::TYPE_1)
            .tiling(vk::ImageTiling::OPTIMAL)
            .usage(vk::ImageUsageFlags::SAMPLED)
            .sharing_mode(vk::SharingMode::EXCLUSIVE)
            .push_next(&mut external_image);
        let image = device.create_image(&image_info, None).ok()?;
        let imported = (|| {
            let requirements = device.get_image_memory_requirements(image);
            let allowed = requirements.memory_type_bits & handle_properties.memory_type_bits;
            let memory_type = (0..32).find(|index| allowed & (1 << index) != 0)?;
            let mut dedicated = vk::MemoryDedicatedAllocateInfo::default().image(image);
            let mut import = vk::ImportMemoryWin32HandleInfoKHR::default()
                .handle_type(handle_type)
                .handle(handle.0 as _);
            // D3D12_RESOURCE import obtains its allocation size from the OS.
            // The borrowed NT handle remains owned by GStreamer.
            let allocation = vk::MemoryAllocateInfo::default()
                .allocation_size(requirements.size)
                .memory_type_index(memory_type)
                .push_next(&mut dedicated)
                .push_next(&mut import);
            let memory = device.allocate_memory(&allocation, None).ok()?;
            if device.bind_image_memory(image, memory, 0).is_err() {
                device.free_memory(memory, None);
                return None;
            }
            let texture_desc = wgpu::hal::TextureDescriptor {
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
            Some(hal.texture_from_raw(
                image,
                &texture_desc,
                None,
                wgpu::hal::vulkan::TextureMemory::Dedicated(memory),
            ))
        })();
        if imported.is_none() {
            device.destroy_image(image, None);
        }
        imported
    }
}
