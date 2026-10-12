mod encoded_scene;
mod encoder;
mod pipelines;
mod resources;
mod runtime;
mod scene;
mod scene_builder;
mod surface;
mod text;
mod video_upload;

use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

pub(crate) use text::TextFrameCache;

/// All blending and effects operate in scene-linear light with enough headroom
/// for grading and glow. Conversion to display encoding happens only in the final pass.
const SCENE_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;
pub(in crate::engine) const OUTPUT_FORMAT: wgpu::TextureFormat =
    wgpu::TextureFormat::Rgba8UnormSrgb;
/// Decoder and text bytes are display-encoded sRGB. Sampling this format performs
/// the input transfer-function decode into scene-linear shader values.
const VIDEO_FRAME_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;
const PROPERTY_WORD_SIZE: usize = size_of::<u32>();
const COMPOSITE: &str = include_str!("composite.wgsl");

use crate::engine::gstreamer::gpu::NativeVideoPipeline;
pub(in crate::engine) use pipelines::VideoInputPipeline;
use pipelines::{ComputePipeline, RasterPipeline, TexturePipeline};
use resources::{FrameResources, NodeResources, VideoTextureCache};
pub(crate) use runtime::RenderRuntime;
pub(crate) use scene::{RenderError, RenderScene, RenderSize};
use zerium_shader::{EffectShaderId, ItemShaderId};

/// Immutable GPU state. A single device can cheaply create independent render
/// sessions for preview, export, thumbnails, and background jobs.
pub(crate) struct RendererDevice {
    device: Arc<wgpu::Device>,
    queue: Arc<wgpu::Queue>,
    pipeline_layout: wgpu::PipelineLayout,
    pipelines: HashMap<ItemShaderId, RasterPipeline>,
    item_bind_group_layout: wgpu::BindGroupLayout,
    capability_bind_group_layout: wgpu::BindGroupLayout,
    effect_bind_group_layout: wgpu::BindGroupLayout,
    temporal_bind_group_layout: wgpu::BindGroupLayout,
    compute_bind_group_layout: wgpu::BindGroupLayout,
    effect_pipeline_layout: wgpu::PipelineLayout,
    temporal_pipeline_layout: wgpu::PipelineLayout,
    composite_bind_group_layout: wgpu::BindGroupLayout,
    backdrop_bind_group_layout: wgpu::BindGroupLayout,
    effect_pipelines: HashMap<EffectShaderId, RasterPipeline>,
    temporal_pipelines: HashMap<EffectShaderId, RasterPipeline>,
    compute_pipelines: HashMap<EffectShaderId, ComputePipeline>,
    composite_pipeline: wgpu::RenderPipeline,
    blend_pipeline: wgpu::RenderPipeline,
    output_pipeline: wgpu::RenderPipeline,
    texture_pipelines: HashMap<ItemShaderId, TexturePipeline>,
    sampler: wgpu::Sampler,
    yuv_upload: video_upload::YuvUploadPipeline,
    native_video: NativeVideoPipeline,
}

/// Mutable rendering session. Resource pools and uploaded-frame reuse are never
/// shared between concurrent consumers.
pub(crate) struct FrameRenderer {
    shared: Arc<RendererDevice>,
    frame_resources: Mutex<Option<FrameResources>>,
    local_resources: Mutex<Vec<NodeResources>>,
    video_textures: Mutex<VideoTextureCache>,
}

pub(crate) struct RendererBuilder {
    device: RendererDevice,
}

impl RendererDevice {
    pub(crate) fn create_session(self: &Arc<Self>) -> FrameRenderer {
        FrameRenderer {
            shared: self.clone(),
            frame_resources: Mutex::new(None),
            local_resources: Mutex::new(Vec::new()),
            video_textures: Mutex::new(VideoTextureCache::default()),
        }
    }
}

impl FrameRenderer {
    pub(in crate::engine) fn gpu(&self) -> (&wgpu::Device, &wgpu::Queue) {
        (&self.shared.device, &self.shared.queue)
    }

    pub(crate) fn fork(&self) -> Self {
        self.shared.create_session()
    }
}
