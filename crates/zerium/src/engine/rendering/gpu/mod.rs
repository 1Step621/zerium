//! Shared GPU pipelines and independent resource ownership for each consumer.
mod bindings;
mod builder;
mod commands;
mod conversion;
mod passes;
mod pipelines;
mod plan;
mod resources;
mod shaders;
mod textures;
mod video_planes;
mod video_upload;

use super::RenderError;
use super::scene;
use crate::engine::gpu::GpuContext;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use conversion::YuvConversionPipeline;
use pipelines::{ComputePipeline, RasterPipeline, TexturePipeline};
use resources::{FrameResources, NodeResources};
use textures::TextureResources;
use video_planes::VideoPlanePipeline;
use zerium_shader::{EffectShaderId, ItemShaderId};

/// All blending and effects operate in scene-linear light with enough headroom
/// for grading and glow. Conversion to display encoding happens only in the final pass.
const SCENE_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;
pub(in crate::engine) const OUTPUT_FORMAT: wgpu::TextureFormat =
    wgpu::TextureFormat::Rgba8UnormSrgb;
/// Decoder and text bytes are display-plan sRGB. Sampling this format performs
/// the input transfer-function decode into scene-linear shader values.
const INPUT_FRAME_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;
const PROPERTY_WORD_SIZE: usize = size_of::<u32>();
const COMPOSITE: &str = include_str!("composite.wgsl");

/// Immutable GPU state. A single device can cheaply create independent render
/// sessions for preview, export, thumbnails, and background jobs.
pub(crate) struct RenderDevice {
    gpu: GpuContext,
    pipeline_layout: wgpu::PipelineLayout,
    item_pipelines: HashMap<ItemShaderId, RasterPipeline>,
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
    video_planes: VideoPlanePipeline,
}

/// Mutable rendering session. Resource pools and uploaded-frame reuse are never
/// shared between concurrent consumers.
pub(crate) struct RenderSession {
    shared: Arc<RenderDevice>,
    frame_resources: Mutex<Option<FrameResources>>,
    local_resources: Mutex<Vec<NodeResources>>,
    texture_resources: Mutex<TextureResources>,
}

pub(crate) struct RenderDeviceBuilder {
    render_device: RenderDevice,
}

impl RenderDevice {
    pub(crate) fn create_session(self: &Arc<Self>) -> RenderSession {
        RenderSession {
            shared: self.clone(),
            frame_resources: Mutex::new(None),
            local_resources: Mutex::new(Vec::new()),
            texture_resources: Mutex::new(TextureResources::default()),
        }
    }
}

impl RenderSession {
    pub(in crate::engine) fn gpu(&self) -> &GpuContext {
        &self.shared.gpu
    }

    pub(crate) fn fork(&self) -> Self {
        self.shared.create_session()
    }
}
