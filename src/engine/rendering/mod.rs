mod encoded_scene;
mod encoder;
mod pipelines;
mod readback;
mod resources;
mod scene;
mod shader;

use std::{
    borrow::Cow,
    collections::{HashMap, HashSet},
    error::Error,
    fmt,
    num::NonZeroU64,
    ops::{Deref, DerefMut, Range},
    sync::{Arc, Mutex},
};

mod text;
pub(crate) use text::TextFrameCache;

use crate::{
    domain::{
        plugin::{
            ComputeDispatchDimension, EffectPassSchema, EffectSchema, ItemSchema, PluginRegistry,
            VisualCapability,
        },
        timeline::{
            EffectInstance, EvaluatedSceneNode, EvaluatedSceneNodeKind, ItemId, LayerId,
            ProjectResolution, TimelineItem, TimelineTime, TimelineView,
        },
    },
    engine::frame::RgbaFrame,
};
use bytemuck::{Pod, Zeroable};

/// All blending and effects operate in scene-linear light with enough headroom
/// for grading and glow. Conversion to display encoding happens only in the final pass.
const SCENE_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;
const OUTPUT_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;
/// Decoder and text bytes are display-encoded sRGB. Sampling this format performs
/// the input transfer-function decode into scene-linear shader values.
const VIDEO_FRAME_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;
const PARAM_WORD_SIZE: usize = size_of::<u32>();
const ITEM_INTERFACE: &str = concat!(
    include_str!("interfaces/item.wgsl"),
    include_str!("interfaces/common.wgsl")
);
const EFFECT_INTERFACE: &str = concat!(
    include_str!("interfaces/effect.wgsl"),
    include_str!("interfaces/common.wgsl")
);
const COMPUTE_INTERFACE: &str = concat!(
    include_str!("interfaces/compute.wgsl"),
    include_str!("interfaces/common.wgsl")
);
const TEMPORAL_INTERFACE: &str = concat!(
    include_str!("interfaces/temporal.wgsl"),
    include_str!("interfaces/common.wgsl")
);
const COMPOSITE: &str = include_str!("composite.wgsl");

pub(crate) use readback::ExportFramePipeline;
pub(crate) use scene::{
    ItemParams, RenderEffect, RenderEffectPass, RenderItem, RenderNode, RenderNodeContent,
    RenderNodeMetadata, RenderScene, RenderTemporalSample,
};
pub(crate) use scene::{RenderError, RenderSize};
use shader::{
    EffectShaderDescriptor, EffectShaderId, ItemShaderDescriptor, ItemShaderId, TextureShaderId,
};

use encoded_scene::*;
use shader::{
    texture_input_ids, texture_media_interface, validate_compute_shader, validate_plugin_shaders,
    validate_render_shader,
};

/// Immutable GPU state. A single device can cheaply create independent render
/// sessions for preview, export, thumbnails, and background jobs.
pub(crate) struct RendererDevice {
    device: Arc<wgpu::Device>,
    queue: Arc<wgpu::Queue>,
    pipeline_layout: wgpu::PipelineLayout,
    pipelines: HashMap<ItemShaderId, ItemPipeline>,
    item_bind_group_layout: wgpu::BindGroupLayout,
    effect_bind_group_layout: wgpu::BindGroupLayout,
    temporal_bind_group_layout: wgpu::BindGroupLayout,
    compute_bind_group_layout: wgpu::BindGroupLayout,
    effect_pipeline_layout: wgpu::PipelineLayout,
    temporal_pipeline_layout: wgpu::PipelineLayout,
    composite_bind_group_layout: wgpu::BindGroupLayout,
    effect_pipelines: HashMap<EffectShaderId, EffectPipeline>,
    temporal_pipelines: HashMap<EffectShaderId, EffectPipeline>,
    compute_pipelines: HashMap<EffectShaderId, ComputePipeline>,
    composite_pipeline: wgpu::RenderPipeline,
    output_pipeline: wgpu::RenderPipeline,
    texture_pipelines: HashMap<TextureShaderId, TexturePipeline>,
    sampler: wgpu::Sampler,
}

/// Mutable rendering session. Resource pools and uploaded-frame reuse are never
/// shared between concurrent consumers.
pub(crate) struct FrameRenderer {
    shared: Arc<RendererDevice>,
    resources: Mutex<HashMap<u32, RenderResources>>,
    video_textures: Mutex<VideoTextureCache>,
}

impl RendererDevice {
    pub(crate) fn create_session(self: &Arc<Self>) -> FrameRenderer {
        FrameRenderer {
            shared: self.clone(),
            resources: Mutex::new(HashMap::new()),
            video_textures: Mutex::new(VideoTextureCache::default()),
        }
    }
}

impl FrameRenderer {
    pub(crate) fn fork(&self) -> Self {
        self.shared.create_session()
    }
}

impl Deref for FrameRenderer {
    type Target = RendererDevice;

    fn deref(&self) -> &Self::Target {
        &self.shared
    }
}

impl DerefMut for FrameRenderer {
    fn deref_mut(&mut self) -> &mut Self::Target {
        Arc::get_mut(&mut self.shared)
            .expect("renderer device is uniquely owned while pipelines are registered")
    }
}

struct ItemPipeline {
    pipeline: wgpu::RenderPipeline,
    vertex_count: u32,
}

struct EffectPipeline {
    pipeline: wgpu::RenderPipeline,
    vertex_count: u32,
}

struct ComputePipeline {
    pipeline: wgpu::ComputePipeline,
    workgroup_size: [u32; 3],
}

struct TexturePipeline {
    pipeline: wgpu::RenderPipeline,
    vertex_count: u32,
    bind_group_layout: wgpu::BindGroupLayout,
    input_ids: Vec<String>,
}

struct TextureResource {
    _uploaded_frames: Vec<Arc<UploadedVideoFrame>>,
    _input_parameters: wgpu::Buffer,
    _item: wgpu::Buffer,
    _item_params: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
}

struct UploadedVideoFrame {
    frame: Arc<RgbaFrame>,
    _texture: wgpu::Texture,
    view: wgpu::TextureView,
}

#[derive(Default)]
struct VideoTextureCache {
    // Adjacent project ticks can refer to the same native video frame. Keeping
    // only the previous scene avoids uploading it twice without mirroring the
    // much larger CPU frame cache in GPU memory.
    previous_scene: Vec<Arc<UploadedVideoFrame>>,
}

struct RenderResources {
    size: RenderSize,
    output_size: RenderSize,
    composition_size: RenderSize,
    item_capacity: usize,
    params_capacity: usize,
    item_buffer: wgpu::Buffer,
    params_buffer: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    effect_instance_stride: u64,
    effect_instance_capacity: usize,
    effect_params_capacity: usize,
    effect_instance_buffer: wgpu::Buffer,
    effect_params_buffer: wgpu::Buffer,
    compute_info_stride: u64,
    compute_info_buffer: wgpu::Buffer,
    compute_inputs: [wgpu::BindGroup; 2],
    _composite_info_buffer: wgpu::Buffer,
    scene_view: wgpu::TextureView,
    output_input: wgpu::BindGroup,
    effect_texture_a: wgpu::Texture,
    effect_texture_b: wgpu::Texture,
    effect_source_texture: wgpu::Texture,
    effect_view_a: wgpu::TextureView,
    effect_view_b: wgpu::TextureView,
    effect_input_a: wgpu::BindGroup,
    effect_input_b: wgpu::BindGroup,
    composite_input_a: wgpu::BindGroup,
    composite_input_b: wgpu::BindGroup,
    compositions: Vec<CompositionRenderResource>,
    temporal: Vec<TemporalRenderResource>,
}

struct CompositionRenderResource {
    texture: wgpu::Texture,
    view: wgpu::TextureView,
}

struct TemporalRenderResource {
    texture_a: wgpu::Texture,
    texture_b: wgpu::Texture,
    view_a: wgpu::TextureView,
    view_b: wgpu::TextureView,
    inputs: Vec<wgpu::BindGroup>,
}

#[derive(Clone, Copy)]
struct RenderResourceRequirements {
    item_count: usize,
    params_size: usize,
    effect_pass_count: usize,
    effect_params_size: usize,
    composition_depth: usize,
    temporal_depth: usize,
}
