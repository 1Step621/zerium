//! Queue ownership and layouts for Vulkan images shared with native decoders.
use ash::vk;

pub(super) fn transfer_ownership(
    device: &wgpu::Device,
    encoder: &mut wgpu::CommandEncoder,
    planes: &[wgpu::Texture],
    acquire: bool,
) {
    // This encoder must contain only raw commands. The caller submits it before
    // or after wgpu's managed command buffer and retains the surfaces until done.
    unsafe {
        let Some(hal) = device.as_hal::<wgpu::hal::api::Vulkan>() else {
            return;
        };
        let family = hal.queue_family_index();
        encoder.as_hal_mut::<wgpu::hal::api::Vulkan, _, _>(|encoder| {
            let Some(encoder) = encoder else { return };
            for plane in planes {
                let Some(texture) = plane.as_hal::<wgpu::hal::api::Vulkan>() else {
                    continue;
                };
                let (source, destination, old, new, src_access, dst_access, src_stage, dst_stage) =
                    if acquire {
                        (
                            vk::QUEUE_FAMILY_EXTERNAL,
                            family,
                            vk::ImageLayout::GENERAL,
                            vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL,
                            vk::AccessFlags::empty(),
                            vk::AccessFlags::SHADER_READ,
                            vk::PipelineStageFlags::TOP_OF_PIPE,
                            vk::PipelineStageFlags::FRAGMENT_SHADER,
                        )
                    } else {
                        (
                            family,
                            vk::QUEUE_FAMILY_EXTERNAL,
                            vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL,
                            vk::ImageLayout::GENERAL,
                            vk::AccessFlags::SHADER_READ,
                            vk::AccessFlags::empty(),
                            vk::PipelineStageFlags::FRAGMENT_SHADER,
                            vk::PipelineStageFlags::BOTTOM_OF_PIPE,
                        )
                    };
                let barrier = vk::ImageMemoryBarrier::default()
                    .image(texture.raw_handle())
                    .src_queue_family_index(source)
                    .dst_queue_family_index(destination)
                    .old_layout(old)
                    .new_layout(new)
                    .src_access_mask(src_access)
                    .dst_access_mask(dst_access)
                    .subresource_range(vk::ImageSubresourceRange {
                        aspect_mask: vk::ImageAspectFlags::COLOR,
                        base_mip_level: 0,
                        level_count: 1,
                        base_array_layer: 0,
                        layer_count: 1,
                    });
                hal.raw_device().cmd_pipeline_barrier(
                    encoder.raw_handle(),
                    src_stage,
                    dst_stage,
                    vk::DependencyFlags::empty(),
                    &[],
                    &[],
                    &[barrier],
                );
            }
        });
    }
}
