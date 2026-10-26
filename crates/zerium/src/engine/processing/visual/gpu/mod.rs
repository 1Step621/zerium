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
