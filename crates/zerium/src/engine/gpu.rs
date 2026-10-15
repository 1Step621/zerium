//! The device and queue shared by a rendering or media consumer.
use gpu_video::VideoAdapterExt;

#[derive(Clone)]
pub(crate) struct GpuContext {
    pub(crate) device: wgpu::Device,
    pub(crate) queue: wgpu::Queue,
}

impl GpuContext {
    pub(crate) fn new(device: wgpu::Device, queue: wgpu::Queue) -> Self {
        Self { device, queue }
    }

    pub(crate) fn request(
        adapter: &wgpu::Adapter,
        descriptor: &wgpu::DeviceDescriptor<'_>,
    ) -> Result<Self, wgpu::RequestDeviceError> {
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
        pollster::block_on(adapter.request_device(descriptor))
            .map(|(device, queue)| Self::new(device, queue))
    }
}
