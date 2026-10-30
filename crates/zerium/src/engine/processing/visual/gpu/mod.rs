//! GPU device creation, frame planning, and consumer rendering sessions.
pub(super) mod device;
mod plan;
pub(super) mod session;

/// All blending and effects operate in scene-linear light with enough headroom
/// for grading and glow. Conversion to display encoding happens only in the final pass.
pub(in crate::engine) const SCENE_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;
pub(in crate::engine) const OUTPUT_FORMAT: wgpu::TextureFormat =
    wgpu::TextureFormat::Rgba8UnormSrgb;

/// Decoder and text bytes are display-plan sRGB. Sampling this format performs
/// the input transfer-function decode into scene-linear shader values.
const INPUT_FRAME_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;
const PROPERTY_WORD_SIZE: usize = size_of::<u32>();

pub(in crate::engine) fn texture_2d(
    device: &wgpu::Device,
    size: super::scene::RenderSize,
    format: wgpu::TextureFormat,
    usage: wgpu::TextureUsages,
    label: &str,
) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d {
            width: size.width,
            height: size.height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage,
        view_formats: &[],
    })
}
